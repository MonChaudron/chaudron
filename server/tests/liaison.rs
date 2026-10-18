//! Liaison de compte Alexa : Chaudron serveur OAuth2 minimal (code d'autorisation + renouvellement). L'écran
//! « Autoriser Alexa » de l'appli appelle `/oauth/client` puis `/oauth/authorize` ; Amazon échange le code sur
//! `/oauth/token`. Les jetons d'accès durent une heure, droits `read` et `cooking:write`, sur un foyer.

mod common;

use axum::http::StatusCode;
use chaudron_server::config::SignupPolicy;
use common::{ALEXA_ID, ALEXA_REDIRECT, ALEXA_SECRET, Browser, TestApp};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const TOKEN: &str = "/api/v1/oauth/token";

/// Alice et son foyer, avec une recette pour cuisiner.
async fn home(app: &TestApp) -> (Browser, Uuid) {
    let mut alice = app.browser();
    alice.login("alice@example.org").await;
    let home = alice.household("Maison").await;
    let recipe = json!({
        "title": "Chili",
        "ingredients": [{"ingredient": "rice", "qty": 50, "unit": "g", "role": "starch"}],
        "variants": [{"label": "Cocotte", "active_min": 20, "total_min": 60,
                      "phases": [{"title": "Cuire", "text": "Cuire."}]}],
    });
    let response = alice
        .put(&format!("/api/v1/households/{home}/recipes/chili"), recipe)
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    (alice, home)
}

/// « Autoriser » sur l'écran de l'appli : renvoie le code reçu par Amazon.
async fn authorize(user: &mut Browser, home: Uuid) -> String {
    let response = user
        .post(
            "/api/v1/oauth/authorize",
            json!({"client_id": ALEXA_ID, "redirect_uri": ALEXA_REDIRECT, "state": "xyz", "household_id": home}),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let target = response.json["redirect_to"].as_str().unwrap();
    let (base, query) = target.split_once('?').unwrap();
    assert_eq!(base, ALEXA_REDIRECT);
    let params: Vec<(String, String)> = serde_urlencoded::from_str(query).unwrap();
    let get = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    assert_eq!(get("state").as_deref(), Some("xyz"));
    get("code").unwrap()
}

async fn exchange(app: &TestApp, code: &str) -> common::Response {
    app.form(
        TOKEN,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", ALEXA_REDIRECT),
        ],
        Some((ALEXA_ID, ALEXA_SECRET)),
    )
    .await
}

#[sqlx::test]
async fn liaison_complete(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Closed).await;
    let (mut alice, home) = home(&app).await;

    // L'écran présente le client et ses droits.
    let response = alice
        .get(&format!(
            "/api/v1/oauth/client?{}",
            serde_urlencoded::to_string([
                ("client_id", ALEXA_ID),
                ("redirect_uri", ALEXA_REDIRECT)
            ])
            .unwrap()
        ))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    assert_eq!(response.json["name"], "Alexa");
    assert_eq!(response.json["scopes"], json!(["read", "cooking:write"]));

    let code = authorize(&mut alice, home).await;
    let response = exchange(&app, &code).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    assert_eq!(response.headers["cache-control"], "no-store");
    let tokens = response.json;
    assert_eq!(tokens["token_type"], "Bearer");
    assert_eq!(tokens["expires_in"], 3600);
    let access = tokens["access_token"].as_str().unwrap().to_owned();
    let refresh = tokens["refresh_token"].as_str().unwrap().to_owned();

    // Le jeton dit son foyer, lit, et fait avancer la cuisson ; il n'écrit pas les recettes.
    let mut skill = app.api_client(&access);
    let response = skill.get("/api/v1/token").await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    assert_eq!(response.json["household_id"], home.to_string());
    assert_eq!(response.json["scopes"], json!(["read", "cooking:write"]));
    let cooking = format!("/api/v1/households/{home}/cooking");
    let response = skill
        .put(
            &cooking,
            json!({"recipe": "chili", "mode": "fast", "phase": 0}),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let response = skill
        .delete(&format!("/api/v1/households/{home}/recipes/chili"))
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);

    // Un code ne sert qu'une fois.
    let response = exchange(&app, &code).await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert_eq!(response.json["error"], "invalid_grant");

    // Une heure plus tard, le jeton d'accès ne vaut plus ; le renouvellement en donne un autre.
    sqlx::query("UPDATE api_tokens SET expires_at = now() - interval '1 second' WHERE expires_at IS NOT NULL")
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        skill.get("/api/v1/token").await.status,
        StatusCode::UNAUTHORIZED
    );
    let response = app
        .form(
            TOKEN,
            &[("grant_type", "refresh_token"), ("refresh_token", &refresh)],
            Some((ALEXA_ID, ALEXA_SECRET)),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    // Sans rotation : Amazon garde le jeton de renouvellement qu'il a.
    assert_eq!(response.json["refresh_token"], Value::Null);
    let access = response.json["access_token"].as_str().unwrap().to_owned();
    assert_eq!(
        app.api_client(&access).get("/api/v1/token").await.status,
        StatusCode::OK
    );
    // Les jetons d'accès expirés de la liaison sont effacés.
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM api_tokens WHERE expires_at IS NOT NULL")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(count, 1);

    // Identifiants du client aussi acceptés dans le corps.
    let response = app
        .form(
            TOKEN,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", &refresh),
                ("client_id", ALEXA_ID),
                ("client_secret", ALEXA_SECRET),
            ],
            None,
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);

    // « Accès » : la liaison est listée à part des jetons, et se retire.
    let links = format!("/api/v1/households/{home}/links");
    let response = alice.get(&links).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let list = response.json.as_array().unwrap().clone();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["client"], "Alexa");
    assert_eq!(list[0]["user"], "alice");
    assert!(list[0]["last_used_at"].is_string());
    assert_eq!(
        alice
            .get(&format!("/api/v1/households/{home}/tokens"))
            .await
            .json,
        json!([])
    );
    let id = list[0]["id"].as_str().unwrap();
    assert_eq!(
        alice.delete(&format!("{links}/{id}")).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.api_client(&access).get("/api/v1/token").await.status,
        StatusCode::UNAUTHORIZED
    );
    let response = app
        .form(
            TOKEN,
            &[("grant_type", "refresh_token"), ("refresh_token", &refresh)],
            Some((ALEXA_ID, ALEXA_SECRET)),
        )
        .await;
    assert_eq!(response.json["error"], "invalid_grant");
}

#[sqlx::test]
async fn refus(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Open).await;
    let (mut alice, home) = home(&app).await;

    // Adresse de retour inconnue, client inconnu : rien n'est envoyé ailleurs.
    for (client, redirect) in [
        (ALEXA_ID, "https://evil.example/cb"),
        ("autre", ALEXA_REDIRECT),
    ] {
        let response = alice
            .post(
                "/api/v1/oauth/authorize",
                json!({"client_id": client, "redirect_uri": redirect, "state": "s", "household_id": home}),
            )
            .await;
        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{client} {redirect} : {}",
            response.text
        );
    }

    // Sans session : pas d'autorisation.
    let mut anonymous = app.browser();
    let response = anonymous
        .post(
            "/api/v1/oauth/authorize",
            json!({"client_id": ALEXA_ID, "redirect_uri": ALEXA_REDIRECT, "state": "s", "household_id": home}),
        )
        .await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);

    // Le foyer d'un autre : 404.
    let mut eve = app.browser();
    eve.login("eve@example.org").await;
    let response = eve
        .post(
            "/api/v1/oauth/authorize",
            json!({"client_id": ALEXA_ID, "redirect_uri": ALEXA_REDIRECT, "state": "s", "household_id": home}),
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);

    // Un membre (pas propriétaire) peut lier Alexa, mais pas gérer les liaisons.
    let invite = alice.invite(home).await;
    let mut bob = app.browser();
    bob.login_with("bob@example.org", Some(&invite)).await;
    let code = authorize(&mut bob, home).await;

    // Mauvais secret, mauvaise adresse de retour, code expiré, type inconnu.
    let response = app
        .form(
            TOKEN,
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", ALEXA_REDIRECT),
            ],
            Some((ALEXA_ID, "faux")),
        )
        .await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert_eq!(response.json["error"], "invalid_client");
    let response = app
        .form(
            TOKEN,
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                (
                    "redirect_uri",
                    "https://pitangui.amazon.com/api/skill/link/M2TEST",
                ),
            ],
            Some((ALEXA_ID, ALEXA_SECRET)),
        )
        .await;
    assert_eq!(response.json["error"], "invalid_grant");
    sqlx::query("UPDATE oauth_grants SET code_expires_at = now() - interval '1 second'")
        .execute(&db)
        .await
        .unwrap();
    let response = exchange(&app, &code).await;
    assert_eq!(response.json["error"], "invalid_grant");
    let response = app
        .form(
            TOKEN,
            &[("grant_type", "password")],
            Some((ALEXA_ID, ALEXA_SECRET)),
        )
        .await;
    assert_eq!(response.json["error"], "unsupported_grant_type");

    let code = authorize(&mut bob, home).await;
    assert_eq!(exchange(&app, &code).await.status, StatusCode::OK);
    assert_eq!(
        bob.get(&format!("/api/v1/households/{home}/links"))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    // Relier remplace la liaison précédente du même compte.
    let code = authorize(&mut bob, home).await;
    assert_eq!(exchange(&app, &code).await.status, StatusCode::OK);
    let links = alice
        .get(&format!("/api/v1/households/{home}/links"))
        .await
        .json;
    assert_eq!(links.as_array().unwrap().len(), 1, "{links}");
    assert_eq!(links[0]["user"], "bob");

    // `/token` ne vaut que pour un jeton.
    assert_eq!(
        alice.get("/api/v1/token").await.status,
        StatusCode::UNAUTHORIZED
    );
}
