use std::{path::PathBuf, time::Duration};

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use chaudron_server::{AppState, app};
use http_body_util::BodyExt;
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;

fn web_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web")
}

/// Une base qui ne répond jamais : pour les routes qui n'en ont pas besoin, et pour le cas dégradé.
fn unreachable_db() -> PgPool {
    PgPoolOptions::new()
        .acquire_timeout(Duration::from_millis(200))
        .connect_lazy("postgres://chaudron@127.0.0.1:1/chaudron")
        .unwrap()
}

async fn get(app: Router, uri: &str) -> (StatusCode, String) {
    let response = app
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

#[sqlx::test]
async fn health_ok_quand_la_base_repond(db: PgPool) {
    let (status, body) = get(app(AppState { db }, &web_dir()), "/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"status":"ok","database":"ok"}"#);
}

#[tokio::test]
async fn health_503_quand_la_base_est_injoignable() {
    let (status, body) = get(
        app(
            AppState {
                db: unreachable_db(),
            },
            &web_dir(),
        ),
        "/health",
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body, r#"{"status":"degraded","database":"unreachable"}"#);
}

#[tokio::test]
async fn openapi_decrit_les_routes() {
    let (status, body) = get(
        app(
            AppState {
                db: unreachable_db(),
            },
            &web_dir(),
        ),
        "/api/v1/openapi.json",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let doc: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(doc["openapi"].as_str().unwrap().starts_with("3."));
    assert!(doc["paths"]["/health"]["get"].is_object());
    assert!(doc["components"]["schemas"]["Health"].is_object());
}

#[tokio::test]
async fn page_d_accueil_servie() {
    let (status, body) = get(
        app(
            AppState {
                db: unreachable_db(),
            },
            &web_dir(),
        ),
        "/",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<title>Chaudron</title>"));
}

#[tokio::test]
async fn route_inconnue_de_l_appli_renvoie_index() {
    let (status, body) = get(
        app(
            AppState {
                db: unreachable_db(),
            },
            &web_dir(),
        ),
        "/cette-semaine",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<title>Chaudron</title>"));
}

#[tokio::test]
async fn route_inconnue_de_l_api_renvoie_404() {
    let (status, _) = get(
        app(
            AppState {
                db: unreachable_db(),
            },
            &web_dir(),
        ),
        "/api/v1/inconnue",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
