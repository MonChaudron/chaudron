//! Serveur Chaudron : API JSON et fichiers de l'appli web.

pub mod auth;
pub mod catalog;
pub mod config;
pub mod crypto;
pub mod error;
pub mod households;
pub mod http;
pub mod members;
pub mod types;
pub mod validate;

use std::{path::Path, sync::Arc};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    middleware,
    routing::{any, get},
};
use serde::Serialize;
use sqlx::PgPool;
use tower_http::services::{ServeDir, ServeFile};
use utoipa::{
    Modify, OpenApi, ToSchema,
    openapi::security::{ApiKey, ApiKeyValue, SecurityScheme},
};

use crate::{auth::fake::FakeOAuth, config::Config, error::ApiError, http::HttpClient};

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub http: HttpClient,
}

impl AppState {
    pub fn new(db: PgPool, config: Config) -> Self {
        Self {
            db,
            config: Arc::new(config),
            http: http::client(),
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Chaudron",
        description = "API de Chaudron : recettes, menus et courses du foyer."
    ),
    modifiers(&SessionCookie),
    security(("session" = [])),
    paths(
        health,
        openapi,
        auth::list_providers,
        auth::login,
        auth::callback,
        auth::logout,
        auth::me,
        auth::update_me,
        households::create,
        households::read,
        households::update,
        households::remove,
        households::change_role,
        households::remove_user,
        households::create_invite,
        households::list_invites,
        households::revoke_invite,
        households::read_invite,
        households::accept,
        members::list,
        members::create,
        members::read,
        members::replace,
        members::remove,
        members::defaults,
        catalog::equipment,
        catalog::ingredients,
        catalog::household_equipment,
        catalog::set_household_equipment,
    )
)]
pub struct ApiDoc;

/// Session web : cookie posé par `/api/v1/auth/{provider}/callback` (`chaudron_session` en HTTP).
struct SessionCookie;

impl Modify for SessionCookie {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        openapi
            .components
            .get_or_insert_default()
            .add_security_scheme(
                "session",
                SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new(
                    auth::session_cookie_name(true),
                ))),
            );
    }
}

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
    security(()),
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
    security(()),
    responses((status = 200, description = "Document OpenAPI 3.1"))
)]
async fn openapi() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

/// Routes de l'API, puis l'appli web pour tout le reste (avec repli sur `index.html` pour le routage côté client).
pub fn app(state: AppState, web_dir: &Path) -> Router {
    let web = ServeDir::new(web_dir).fallback(ServeFile::new(web_dir.join("index.html")));
    let api = Router::new()
        .merge(auth::router())
        .merge(households::router())
        .merge(members::router())
        .merge(catalog::router())
        .route("/openapi.json", get(openapi))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::session_layer,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::origin_guard,
        ));
    let mut router = Router::new()
        .route("/health", get(health))
        .nest("/api/v1", api)
        // Jamais l'appli web sous /api : une route inconnue répond une erreur JSON.
        .route("/api/{*rest}", any(|| async { ApiError::not_found() }));
    if state.config.fake_oauth {
        router = router.nest("/dev/oauth", FakeOAuth::default().router());
    }
    router.fallback_service(web).with_state(state)
}
