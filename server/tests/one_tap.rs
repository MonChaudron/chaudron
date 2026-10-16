//! Google One Tap : jetons signés par une clé de test, clés publiques servies comme par Google.

mod common;

use std::time::{SystemTime, UNIX_EPOCH};

use axum::{Json, Router, http::StatusCode, routing::get};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chaudron_server::{
    auth::providers::{Provider, ProviderId},
    config::SignupPolicy,
};
use common::{TestApp, config};
use ring::{
    rand::SystemRandom,
    signature::{RSA_PKCS1_SHA256, RsaKeyPair, RsaPublicKeyComponents},
};
use serde_json::{Value, json};
use sqlx::PgPool;

/// Clé RSA 2048 de test (PKCS#8, générée par openssl) : ne sert qu'aux tests.
const TEST_KEY: &[u8] = include_bytes!("data/one-tap-test-key.pk8");
const CLIENT_ID: &str = "test-client.apps.googleusercontent.com";

fn key() -> RsaKeyPair {
    RsaKeyPair::from_pkcs8(TEST_KEY).unwrap()
}

fn b64(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

/// Sert les clés publiques comme `https://www.googleapis.com/oauth2/v3/certs`.
async fn google_keys() -> String {
    let public: RsaPublicKeyComponents<Vec<u8>> = key().public().into();
    let jwks = json!({"keys": [{
        "kid": "test-1", "kty": "RSA", "alg": "RS256", "use": "sig",
        "n": b64(&public.n),
        "e": b64(&public.e),
    }]});
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new().route("/certs", get(move || async move { Json(jwks) }));
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("{base}/certs")
}

async fn app(db: PgPool, signup: SignupPolicy) -> TestApp {
    let jwks = google_keys().await;
    TestApp::with(db, move |fake_base| {
        let mut config = config(fake_base, signup);
        let mut google = Provider::new(ProviderId::Google, CLIENT_ID.into(), "secret".into());
        google.jwks_url = Some(jwks);
        config.providers.insert(0, google);
        config
    })
    .await
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// Jeton d'identité tel que Google le donnerait à la page.
fn token(claims: Value) -> String {
    let header = b64(br#"{"alg":"RS256","kid":"test-1","typ":"JWT"}"#);
    let payload = b64(claims.to_string().as_bytes());
    let input = format!("{header}.{payload}");
    let key = key();
    let mut signature = vec![0; key.public().modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        input.as_bytes(),
        &mut signature,
    )
    .unwrap();
    format!("{input}.{}", b64(&signature))
}

fn claims(email: &str) -> Value {
    json!({
        "iss": "https://accounts.google.com",
        "aud": CLIENT_ID,
        "sub": format!("google-{email}"),
        "email": email,
        "email_verified": true,
        "name": "Christophe",
        "iat": now(),
        "exp": now() + 3600,
    })
}

async fn one_tap(browser: &mut common::Browser, body: Value) -> common::Response {
    browser.post("/api/v1/auth/google/one-tap", body).await
}

#[sqlx::test]
async fn le_client_google_est_annonce_pour_one_tap(db: PgPool) {
    let app = app(db, SignupPolicy::Closed).await;
    let providers = app.browser().get("/api/v1/auth/providers").await.json;
    assert_eq!(
        providers,
        json!([{"id": "google", "client_id": CLIENT_ID}, {"id": "fake", "client_id": null}])
    );
}

#[sqlx::test]
async fn connexion_one_tap(db: PgPool) {
    let app = app(db, SignupPolicy::Closed).await;
    let mut browser = app.browser();
    let response = one_tap(
        &mut browser,
        json!({"credential": token(claims("christophe@example.org"))}),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    assert_eq!(response.json, json!({"return_to": "/"}));
    let me = browser.me().await;
    assert_eq!(me["display_name"], "Christophe");
    assert_eq!(
        me["identities"],
        json!([{"provider": "google", "email": "christophe@example.org"}])
    );
}

#[sqlx::test]
async fn jetons_refuses(db: PgPool) {
    let app = app(db, SignupPolicy::Open).await;
    let mut browser = app.browser();
    let mut cases = Vec::new();

    let mut other_app = claims("a@example.org");
    other_app["aud"] = json!("autre-appli.apps.googleusercontent.com");
    cases.push(("autre audience", token(other_app)));

    let mut expired = claims("a@example.org");
    expired["exp"] = json!(now() - 3600);
    cases.push(("expiré", token(expired)));

    let mut issuer = claims("a@example.org");
    issuer["iss"] = json!("https://evil.example");
    cases.push(("autre émetteur", token(issuer)));

    // Contenu modifié après signature.
    let genuine = token(claims("a@example.org"));
    let parts: Vec<&str> = genuine.split('.').collect();
    let forged = b64(claims("eve@example.org").to_string().as_bytes());
    cases.push(("signature", format!("{}.{forged}.{}", parts[0], parts[2])));

    // Algorithme « none ».
    let none = format!(
        "{}.{}.",
        b64(br#"{"alg":"none","kid":"test-1"}"#),
        b64(claims("a@example.org").to_string().as_bytes())
    );
    cases.push(("alg none", none));
    cases.push(("pas un jeton", "abc".into()));

    for (case, credential) in cases {
        let response = one_tap(&mut browser, json!({ "credential": credential })).await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED, "{case}");
        assert_eq!(response.json["error"], "one_tap_refused", "{case}");
    }
    assert_eq!(
        browser.get("/api/v1/me").await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn inscriptions_fermees_sauf_invitation(db: PgPool) {
    let app = app(db, SignupPolicy::Closed).await;
    let mut owner = app.browser();
    one_tap(
        &mut owner,
        json!({"credential": token(claims("christophe@example.org"))}),
    )
    .await;
    let home = owner.household("Maison").await;

    let mut stranger = app.browser();
    let response = one_tap(
        &mut stranger,
        json!({"credential": token(claims("eve@example.org"))}),
    )
    .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    assert_eq!(response.json["error"], "signup_closed");

    let invite = owner.invite(home).await;
    let mut guest = app.browser();
    let response = one_tap(
        &mut guest,
        json!({"credential": token(claims("camille@example.org")), "invite": invite}),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    assert_eq!(response.json["return_to"], format!("/invitation/{invite}"));
    assert_eq!(guest.me().await["households"][0]["role"], "member");
}
