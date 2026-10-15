//! Serveur Chaudron : API JSON et fichiers de l'appli web.

use std::path::Path;

use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use serde::Serialize;
use sqlx::PgPool;
use tower_http::services::{ServeDir, ServeFile};
use utoipa::{OpenApi, ToSchema};

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Chaudron",
        description = "API de Chaudron : recettes, menus et courses du foyer."
    ),
    paths(health, openapi)
)]
pub struct ApiDoc;

/// État du serveur et de ses dépendances.
#[derive(Serialize, ToSchema)]
pub struct Health {
    /// `ok` ou `degraded`.
    pub status: &'static str,
    /// `ok` ou `unreachable`.
    pub database: &'static str,
}

/// Healthcheck Docker : répond 503 si la base ne répond pas.
#[utoipa::path(
    get,
    path = "/health",
    tag = "système",
    responses(
        (status = 200, description = "Serveur et base opérationnels", body = Health),
        (status = 503, description = "Base injoignable", body = Health),
    )
)]
async fn health(State(state): State<AppState>) -> (StatusCode, Json<Health>) {
    match sqlx::query("SELECT 1").execute(&state.db).await {
        Ok(_) => (
            StatusCode::OK,
            Json(Health {
                status: "ok",
                database: "ok",
            }),
        ),
        Err(err) => {
            tracing::warn!(%err, "base injoignable");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(Health {
                    status: "degraded",
                    database: "unreachable",
                }),
            )
        }
    }
}

/// Contrat OpenAPI de l'API.
#[utoipa::path(
    get,
    path = "/api/v1/openapi.json",
    tag = "système",
    responses((status = 200, description = "Document OpenAPI 3.1"))
)]
async fn openapi() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

/// Routes de l'API, puis l'appli web pour tout le reste (avec repli sur `index.html` pour le routage côté client).
pub fn app(state: AppState, web_dir: &Path) -> Router {
    let web = ServeDir::new(web_dir).fallback(ServeFile::new(web_dir.join("index.html")));
    Router::new()
        .route("/health", get(health))
        .route("/api/v1/openapi.json", get(openapi))
        .route("/api/{*rest}", get(|| async { StatusCode::NOT_FOUND }))
        .fallback_service(web)
        .with_state(state)
}
