mod common;

use std::{fs, path::Path};

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use chaudron_server::ApiDoc;
use common::{offline_app, unreachable_db};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;
use utoipa::OpenApi;

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
    let (status, body) = get(offline_app(db), "/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"status":"ok","database":"ok"}"#);
}

#[tokio::test]
async fn health_503_quand_la_base_est_injoignable() {
    let (status, body) = get(offline_app(unreachable_db()), "/health").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body, r#"{"status":"degraded","database":"unreachable"}"#);
}

#[tokio::test]
async fn openapi_decrit_les_routes() {
    let (status, body) = get(offline_app(unreachable_db()), "/api/v1/openapi.json").await;
    assert_eq!(status, StatusCode::OK);
    let doc: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(doc["openapi"].as_str().unwrap().starts_with("3."));
    assert!(doc["paths"]["/health"]["get"].is_object());
    assert!(doc["paths"]["/api/v1/me"]["get"].is_object());
    assert!(doc["paths"]["/api/v1/households/{id}/members/{member_id}"]["put"].is_object());
    for schema in [
        "Health",
        "Me",
        "Household",
        "Member",
        "Portions",
        "Equipment",
        "ErrorBody",
    ] {
        assert!(doc["components"]["schemas"][schema].is_object(), "{schema}");
    }
    assert_eq!(
        doc["components"]["securitySchemes"]["session"]["in"],
        "cookie"
    );
    // Les valeurs des énumérations sont celles que l'API envoie vraiment.
    assert_eq!(
        doc["components"]["schemas"]["Role"]["enum"],
        serde_json::json!(["owner", "member"])
    );
    assert_eq!(
        doc["components"]["schemas"]["MemberKind"]["enum"],
        serde_json::json!(["adult", "teen", "child"])
    );
    // Identifiants d'opération uniques : le client TypeScript les utilise comme clés.
    let mut ids: Vec<&str> = doc["paths"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|item| item.as_object().unwrap().values())
        .filter_map(|op| op["operationId"].as_str())
        .collect();
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "operationId en double");
    // Les filtres sont des paramètres de requête, facultatifs sauf la recherche Ciqual.
    for (path, name, required) in [
        ("/api/v1/foods", "q", true),
        ("/api/v1/foods", "limit", false),
        ("/api/v1/households/{id}/recipes", "q", false),
        ("/api/v1/households/{id}/recipes", "tag", false),
    ] {
        let param = doc["paths"][path]["get"]["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap_or_else(|| panic!("{path} {name}"));
        assert_eq!(param["in"], "query", "{path} {name}");
        assert_eq!(param["required"], required, "{path} {name}");
    }
    // Les routes publiques n'exigent pas de session.
    assert_eq!(
        doc["paths"]["/api/v1/auth/providers"]["get"]["security"],
        serde_json::json!([{}])
    );
}

#[tokio::test]
async fn page_d_accueil_servie() {
    let (status, body) = get(offline_app(unreachable_db()), "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<title>Chaudron</title>"));
}

#[tokio::test]
async fn route_inconnue_de_l_appli_renvoie_index() {
    let (status, body) = get(offline_app(unreachable_db()), "/cette-semaine").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<title>Chaudron</title>"));
}

/// En-têtes de cache des fichiers de l'appli : hachés, gardés un an ; la page et le service worker, toujours
/// revalidés (sinon une nouvelle version ne s'installe pas).
#[tokio::test]
async fn cache_des_fichiers_de_l_appli() {
    async fn cache(uri: &str) -> String {
        let response = offline_app(unreachable_db())
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        response.headers()["cache-control"]
            .to_str()
            .unwrap()
            .to_owned()
    }
    assert_eq!(
        cache("/_expo/static/js/entry-0123abcd.js").await,
        "public, max-age=31536000, immutable"
    );
    assert_eq!(cache("/").await, "no-cache");
    assert_eq!(cache("/courses").await, "no-cache");
    assert_eq!(cache("/sw.js").await, "no-cache");
    // Un fichier haché absent tombe sur la page de l'appli : jamais gardée un an (ni par Cloudflare).
    assert_eq!(cache("/_expo/static/js/absent-0123.js").await, "no-cache");
}

#[tokio::test]
async fn route_inconnue_de_l_api_renvoie_404() {
    let (status, body) = get(offline_app(unreachable_db()), "/api/v1/inconnue").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains(r#""error":"not_found""#));
    let (status, _) = get(offline_app(unreachable_db()), "/api/v2/inconnue").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Le client TypeScript est généré depuis `packages/api/openapi.json` : il doit suivre le serveur.
/// Pour le réécrire : `UPDATE_OPENAPI=1 scripts/cargo.sh test --test api`.
#[test]
fn openapi_json_a_jour() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../packages/api/openapi.json");
    let current = serde_json::to_string_pretty(&ApiDoc::openapi()).unwrap()
        + "
";
    if std::env::var("UPDATE_OPENAPI").as_deref() == Ok("1") {
        fs::write(&path, &current).unwrap();
        return;
    }
    let committed = fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == current,
        "packages/api/openapi.json n'est plus à jour : UPDATE_OPENAPI=1 scripts/cargo.sh test --test api"
    );
}
