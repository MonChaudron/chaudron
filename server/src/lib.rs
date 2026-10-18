//! Serveur Chaudron : API JSON et fichiers de l'appli web.

pub mod auth;
pub mod catalog;
pub mod ciqual;
pub mod config;
pub mod cooking;
pub mod crypto;
pub mod error;
pub mod events;
pub mod households;
pub mod http;
pub mod ingredients;
pub mod members;
pub mod menus;
pub mod recipes;
pub mod shopping;
pub mod text;
pub mod tokens;
pub mod types;
pub mod validate;

use std::{path::Path, sync::Arc};

use axum::{
    Json, Router,
    extract::State,
    http::{
        HeaderValue, Method, StatusCode, Uri,
        header::{CACHE_CONTROL, CONTENT_TYPE},
    },
    middleware,
    response::Response,
    routing::{any, get},
};
use serde::Serialize;
use sqlx::PgPool;
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
};
use utoipa::{
    Modify, OpenApi, ToSchema,
    openapi::security::{ApiKey, ApiKeyValue, SecurityScheme},
};

use crate::{
    auth::{fake::FakeOAuth, one_tap::GoogleKeys},
    config::Config,
    error::ApiError,
    http::HttpClient,
};

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub http: HttpClient,
    pub google_keys: Arc<GoogleKeys>,
    /// Événements du foyer, pour le flux SSE.
    pub events: tokio::sync::broadcast::Sender<events::Event>,
}

impl AppState {
    pub fn new(db: PgPool, config: Config) -> Self {
        Self {
            db,
            config: Arc::new(config),
            http: http::client(),
            google_keys: Arc::default(),
            events: events::channel(),
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
        auth::one_tap_login,
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
        ingredients::list_ingredients,
        ingredients::create_ingredient,
        ingredients::link_food,
        ingredients::search_foods,
        recipes::list_recipes,
        recipes::read_recipe,
        recipes::put_recipe,
        recipes::delete_recipe,
        recipes::import,
        menus::list_menus,
        menus::read_menu,
        menus::put_menu,
        menus::delete_menu,
        menus::move_slot,
        menus::freeze,
        menus::agenda,
        shopping::read_list,
        shopping::apply_ops,
        shopping::list_ops,
        cooking::read_cooking,
        cooking::put_cooking,
        cooking::end_cooking,
        events::events,
        tokens::list_tokens,
        tokens::create_token,
        tokens::revoke_token,
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
        .merge(ingredients::router())
        .merge(recipes::router())
        .merge(menus::router())
        .merge(shopping::router())
        .merge(cooking::router())
        .merge(events::router())
        .merge(tokens::router())
        .route("/openapi.json", get(openapi))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::session_layer,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::origin_guard,
        ));
    // Dev seulement : l'appli servie par Expo sur une autre origine appelle l'API avec ses cookies.
    let api = match state.config.dev_web_origin.as_deref() {
        Some(origin) => api.layer(
            CorsLayer::new()
                .allow_origin(HeaderValue::from_str(origin).expect("DEV_WEB_ORIGIN valide"))
                .allow_credentials(true)
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::PATCH,
                    Method::DELETE,
                ])
                .allow_headers([CONTENT_TYPE]),
        ),
        None => api,
    };
    let mut router = Router::new()
        .route("/health", get(health))
        .nest("/api/v1", api)
        // Jamais l'appli web sous /api : une route inconnue répond une erreur JSON.
        .route("/api/{*rest}", any(|| async { ApiError::not_found() }));
    if state.config.fake_oauth {
        router = router.nest("/dev/oauth", FakeOAuth::default().router());
    }
    let web = Router::new()
        .fallback_service(web)
        .layer(middleware::map_response(static_cache));
    router.fallback_service(web).with_state(state)
}

/// Cache des fichiers de l'appli : les fichiers hachés d'Expo ne changent jamais (un an, `immutable`, gardés par
/// Cloudflare) ; la page et le service worker sont toujours revalidés, sinon une nouvelle version ne s'installe pas.
async fn static_cache(uri: Uri, mut response: Response) -> Response {
    let path = uri.path();
    let html = response
        .headers()
        .get(CONTENT_TYPE)
        .is_some_and(|t| t.as_bytes().starts_with(b"text/html"));
    // La page d'abord : un fichier haché absent tombe sur `index.html`, qu'il ne faut jamais garder un an.
    let value = if html || path == "/sw.js" {
        "no-cache"
    } else if path.starts_with("/_expo/static/") || path.starts_with("/assets/") {
        "public, max-age=31536000, immutable"
    } else {
        // Polices, icônes, manifeste : noms stables, revalidés chaque semaine.
        "public, max-age=604800"
    };
    if response.status().is_success() {
        response
            .headers_mut()
            .insert(CACHE_CONTROL, HeaderValue::from_static(value));
    }
    response
}
