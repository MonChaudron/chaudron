//! Connexion de bout en bout avec le fournisseur factice : le serveur échange vraiment le code (PKCE) en HTTP.

mod common;

use axum::http::StatusCode;
use chaudron_server::config::SignupPolicy;
use common::TestApp;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test]
async fn sans_session_l_api_repond_401(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let response = app.browser().get("/api/v1/me").await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert_eq!(response.json["error"], "unauthenticated");
}

#[sqlx::test]
async fn fournisseurs_disponibles(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let response = app.browser().get("/api/v1/auth/providers").await;
    assert_eq!(response.json, json!([{"id": "fake", "client_id": null}]));
    let response = app.browser().get("/api/v1/auth/google/login").await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn premier_compte_puis_inscriptions_fermees(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;

    let mut alice = app.browser();
    assert_eq!(alice.login("alice@example.org").await, "/");
    let me = alice.me().await;
    assert_eq!(me["display_name"], "alice");
    assert_eq!(me["email"], "alice@example.org");
    assert_eq!(
        me["identities"],
        json!([{"provider": "fake", "email": "alice@example.org"}])
    );
    assert_eq!(me["households"], json!([]));

    let mut eve = app.browser();
    assert_eq!(
        eve.login("eve@example.org").await,
        "/connexion?erreur=inscriptions-fermees"
    );
    assert_eq!(eve.get("/api/v1/me").await.status, StatusCode::UNAUTHORIZED);

    // Le compte existant se reconnecte, et retrouve le même compte.
    let mut alice_again = app.browser();
    assert_eq!(alice_again.login("alice@example.org").await, "/");
    assert_eq!(alice_again.me().await["id"], me["id"]);
}

#[sqlx::test]
async fn adresses_autorisees_par_la_configuration(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::parse("bob@example.org")).await;
    app.browser().login("alice@example.org").await;
    assert_eq!(app.browser().login("BOB@example.org").await, "/");
    assert_eq!(
        app.browser().login("eve@example.org").await,
        "/connexion?erreur=inscriptions-fermees"
    );

    // Même base, inscriptions ouvertes.
    let open = TestApp::new(db, SignupPolicy::Open).await;
    assert_eq!(open.browser().login("eve@example.org").await, "/");
}

#[sqlx::test]
async fn avec_une_liste_le_premier_compte_doit_y_figurer(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::parse("alice@example.org")).await;
    assert_eq!(
        app.browser().login("eve@example.org").await,
        "/connexion?erreur=inscriptions-fermees"
    );
    assert_eq!(app.browser().login("alice@example.org").await, "/");
}

#[sqlx::test]
async fn cookie_de_session_httponly_et_lax(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let mut browser = app.browser();
    let callback = browser
        .authorize("/api/v1/auth/fake/login", "alice@example.org")
        .await;
    let response = browser.get(&callback).await;
    let cookies = response.set_cookies();
    assert!(cookies.contains(&"chaudron_oauth=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"));
    let session = cookies
        .iter()
        .find(|c| c.starts_with("chaudron_session="))
        .unwrap();
    assert!(session.ends_with("; Path=/; HttpOnly; SameSite=Lax; Max-Age=31536000"));
}

#[sqlx::test]
async fn retour_vers_la_page_demandee(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let mut browser = app.browser();
    let callback = browser
        .authorize(
            "/api/v1/auth/fake/login?return_to=/foyer",
            "alice@example.org",
        )
        .await;
    assert_eq!(browser.get(&callback).await.location(), "/foyer");

    let callback = browser
        .authorize(
            "/api/v1/auth/fake/login?return_to=//evil.example",
            "alice@example.org",
        )
        .await;
    assert_eq!(browser.get(&callback).await.location(), "/");
}

#[sqlx::test]
async fn state_falsifie_refuse(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Open).await;
    let mut browser = app.browser();
    let callback = browser
        .authorize("/api/v1/auth/fake/login", "alice@example.org")
        .await;
    let (path, query) = callback.split_once('?').unwrap();
    let forged: String = query
        .split('&')
        .map(|p| {
            if p.starts_with("state=") {
                "state=forge".to_owned()
            } else {
                p.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    let response = browser.get(&format!("{path}?{forged}")).await;
    assert_eq!(response.location(), "/connexion?erreur=expiree");
    assert_eq!(
        browser.get("/api/v1/me").await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn sans_cookie_de_connexion_refuse(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Open).await;
    let mut browser = app.browser();
    let callback = browser
        .authorize("/api/v1/auth/fake/login", "alice@example.org")
        .await;
    // Retour ouvert dans un autre navigateur (pas de cookie chaudron_oauth).
    let response = app.browser().get(&callback).await;
    assert_eq!(response.location(), "/connexion?erreur=expiree");
}

#[sqlx::test]
async fn pkce_verifie_par_le_fournisseur(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Open).await;
    let mut browser = app.browser();
    let callback = browser
        .authorize("/api/v1/auth/fake/login", "alice@example.org")
        .await;
    // Un code_verifier qui ne correspond pas au code_challenge envoyé : le fournisseur refuse l'échange.
    let pending = browser.cookies.get("chaudron_oauth").unwrap().clone();
    let tampered: Vec<String> = pending
        .split('&')
        .map(|p| {
            if p.starts_with("verifier=") {
                "verifier=autre".to_owned()
            } else {
                p.to_owned()
            }
        })
        .collect();
    browser
        .cookies
        .insert("chaudron_oauth".into(), tampered.join("&"));
    let response = browser.get(&callback).await;
    assert_eq!(response.location(), "/connexion?erreur=fournisseur");
}

#[sqlx::test]
async fn refus_chez_le_fournisseur(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Open).await;
    let mut browser = app.browser();
    browser.get("/api/v1/auth/fake/login").await;
    let response = browser
        .get("/api/v1/auth/fake/callback?error=access_denied&state=x")
        .await;
    assert_eq!(response.location(), "/connexion?erreur=refus");
}

#[sqlx::test]
async fn deconnexion_revoque_la_session(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let mut browser = app.browser();
    browser.login("alice@example.org").await;
    let token = browser.cookies.get("chaudron_session").unwrap().clone();

    let response = browser.post("/api/v1/auth/logout", json!(null)).await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert!(!browser.cookies.contains_key("chaudron_session"));

    // Même avec l'ancien cookie, la session n'existe plus.
    browser.cookies.insert("chaudron_session".into(), token);
    assert_eq!(
        browser.get("/api/v1/me").await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn session_prolongee_a_l_usage(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Closed).await;
    let mut browser = app.browser();
    browser.login("alice@example.org").await;

    // Une session vue il y a deux jours est prolongée d'un an, et le cookie renvoyé.
    sqlx::query("UPDATE sessions SET last_seen_at = now() - interval '2 days', expires_at = now() + interval '1 day'")
        .execute(&db)
        .await
        .unwrap();
    let response = browser.get("/api/v1/me").await;
    assert_eq!(response.status, StatusCode::OK);
    assert!(response.set_cookies()[0].starts_with("chaudron_session="));
    let days: f64 = sqlx::query_scalar(
        "SELECT extract(epoch FROM expires_at - now())::float8 / 86400 FROM sessions",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert!(days > 364.0);

    // Session expirée : plus connecté.
    sqlx::query("UPDATE sessions SET expires_at = now() - interval '1 second'")
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        browser.get("/api/v1/me").await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn ecriture_depuis_un_autre_site_refusee(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let mut browser = app.browser();
    browser.login("alice@example.org").await;
    browser.origin = Some("https://evil.example".into());
    let response = browser
        .post("/api/v1/households", json!({"name": "Pirate"}))
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    // Sans en-tête Origin (client non navigateur), la session suffit.
    browser.origin = None;
    let response = browser
        .post("/api/v1/households", json!({"name": "Maison"}))
        .await;
    assert_eq!(response.status, StatusCode::CREATED);
}

#[sqlx::test]
async fn lier_un_second_fournisseur_au_compte(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let mut alice = app.browser();
    alice.login("alice@example.org").await;
    let id = alice.me().await["id"].clone();

    // Connectée, Alice se connecte avec une autre identité : elle est liée à son compte.
    assert_eq!(alice.login("alice.perso@example.org").await, "/");
    let me = alice.me().await;
    assert_eq!(me["id"], id);
    assert_eq!(me["identities"].as_array().unwrap().len(), 2);

    // Cette identité ouvre désormais le même compte, malgré les inscriptions fermées.
    let mut other = app.browser();
    assert_eq!(other.login("alice.perso@example.org").await, "/");
    assert_eq!(other.me().await["id"], id);
}

#[sqlx::test]
async fn identite_deja_liee_a_un_autre_compte(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Open).await;
    app.browser().login("bob@example.org").await;
    let mut alice = app.browser();
    alice.login("alice@example.org").await;
    assert_eq!(
        alice.login("bob@example.org").await,
        "/connexion?erreur=deja-lie"
    );
}

#[sqlx::test]
async fn changer_son_nom(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let mut alice = app.browser();
    alice.login("alice@example.org").await;
    let response = alice
        .patch("/api/v1/me", json!({"display_name": "  Alice B. "}))
        .await;
    assert_eq!(response.json["display_name"], "Alice B.");
    let response = alice
        .patch("/api/v1/me", json!({"display_name": " "}))
        .await;
    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test]
async fn appli_de_dev_sur_une_autre_origine(db: PgPool) {
    const DEV: &str = "http://localhost:8081";
    let app = TestApp::with(db, |fake_base| {
        let mut config = common::config(fake_base, SignupPolicy::Closed);
        config.dev_web_origin = Some(DEV.into());
        config
    })
    .await;
    let mut browser = app.browser();
    // Après la connexion, retour sur le serveur de dev de l'appli.
    assert_eq!(browser.login("alice@example.org").await, format!("{DEV}/"));

    // Écritures acceptées depuis l'appli de dev, avec CORS et cookies.
    browser.origin = Some(DEV.into());
    let response = browser
        .post("/api/v1/households", json!({"name": "Maison"}))
        .await;
    assert_eq!(response.status, StatusCode::CREATED);
    assert_eq!(response.headers["access-control-allow-origin"], DEV);
    assert_eq!(response.headers["access-control-allow-credentials"], "true");
    browser.origin = Some("http://localhost:9999".into());
    let response = browser
        .post("/api/v1/households", json!({"name": "Autre"}))
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
}
