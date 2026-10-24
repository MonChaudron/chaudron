//! Apports d'une portion : la note et les apports dans le menu, la recette et la liste des recettes.

mod common;

use axum::http::StatusCode;
use chaudron_server::{ciqual, config::SignupPolicy, prices};
use common::{Browser, TestApp};
use serde_json::{Value, json};
use sqlx::PgPool;

async fn setup(db: &PgPool) -> TestApp {
    ciqual::sync(db).await.unwrap();
    prices::seed_links(db).await.unwrap();
    TestApp::new(db.clone(), SignupPolicy::Open).await
}

async fn home(app: &TestApp) -> (Browser, String) {
    let mut alice = app.browser();
    alice.login("alice@example.org").await;
    let home = alice.household("Maison").await.to_string();
    (alice, home)
}

fn lot() -> Value {
    let phases = json!([{"title": "Cuire", "text": "Cuire."}]);
    json!({
        "recipes": [{"slug": "poulet-riz", "title": "Poulet, riz, carottes", "ingredients": [
            {"ingredient": "chicken", "qty": 150, "unit": "g", "role": "protein"},
            {"ingredient": "rice", "qty": 80, "unit": "g", "role": "starch", "state": "dried"},
            {"ingredient": "carrot", "qty": 200, "unit": "g", "role": "veg"},
        ], "variants": [{"label": "Cocotte", "active_min": 10, "total_min": 30, "phases": phases}]}],
        "menu": {"week_start": "2026-10-18", "slots": [
            {"date": "2026-10-18", "meal": "dinner", "recipe": "poulet-riz", "covers_next_lunch": false},
        ]},
    })
}

async fn published(app: &TestApp) -> (Browser, String) {
    let (mut alice, home) = home(app).await;
    let response = alice
        .post(&format!("/api/v1/households/{home}/import"), lot())
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    (alice, home)
}

async fn token(browser: &mut Browser, home: &str) -> String {
    let response = browser
        .post(
            &format!("/api/v1/households/{home}/tokens"),
            json!({"name": "Claude", "scopes": ["read"]}),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    response.json["token"].as_str().unwrap().to_owned()
}

fn carbs(nutrition: &Value) -> f64 {
    nutrition["amounts"]["carbs_g"].as_f64().unwrap()
}

#[sqlx::test]
async fn menu_porte_les_apports(db: PgPool) {
    let app = setup(&db).await;
    let (mut alice, home) = published(&app).await;
    let menu = alice
        .get(&format!("/api/v1/households/{home}/menus/2026-10-18"))
        .await;
    assert_eq!(menu.status, StatusCode::OK, "{}", menu.text);
    let nutrition = &menu.json["slots"][0]["nutrition"];
    assert!(nutrition["grade"].is_string(), "{nutrition}");
    assert!(nutrition["amounts"]["energy_kcal"].as_f64().unwrap() > 0.0);
    assert_eq!(nutrition["complete"], true, "{nutrition}");
}

#[sqlx::test]
async fn portion_du_membre_relie(db: PgPool) {
    let app = setup(&db).await;
    let (mut alice, home) = published(&app).await;
    let alice_id = alice.me().await["id"].clone();
    let response = alice
        .post(
            &format!("/api/v1/households/{home}/members"),
            json!({"name": "Alice", "kind": "teen", "user_id": alice_id}),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);

    let route = format!("/api/v1/households/{home}/recipes/poulet-riz/nutrition");
    let mine = alice.get(&route).await.json;
    assert_eq!(mine["portion"], "member");
    let standard = app
        .api_client(&token(&mut alice, &home).await)
        .get(&route)
        .await
        .json;
    assert_eq!(standard["portion"], "standard");
    assert!(
        carbs(&mine) > carbs(&standard) + 20.0,
        "l'ado prend double féculent"
    );
    assert_eq!(mine["grade"], standard["grade"]);

    let menu = alice
        .get(&format!("/api/v1/households/{home}/menus/2026-10-18"))
        .await
        .json;
    assert_eq!(menu["slots"][0]["nutrition"]["portion"], "member");
}

#[sqlx::test]
async fn jeton_et_compte_sans_membre_portion_standard(db: PgPool) {
    let app = setup(&db).await;
    let (mut alice, home) = published(&app).await;
    let route = format!("/api/v1/households/{home}/recipes/poulet-riz/nutrition");
    assert_eq!(alice.get(&route).await.json["portion"], "standard");
    let mut claude = app.api_client(&token(&mut alice, &home).await);
    let menu = claude
        .get(&format!("/api/v1/households/{home}/menus/2026-10-18"))
        .await
        .json;
    assert_eq!(menu["slots"][0]["nutrition"]["portion"], "standard");
}

#[sqlx::test]
async fn recette_communaute_notee(db: PgPool) {
    let app = setup(&db).await;
    let (mut alice, home) = published(&app).await;
    sqlx::query("UPDATE recipes SET household_id = NULL WHERE slug = 'poulet-riz'")
        .execute(&db)
        .await
        .unwrap();
    let menu = alice
        .get(&format!("/api/v1/households/{home}/menus/2026-10-18"))
        .await
        .json;
    assert!(menu["slots"][0]["nutrition"]["grade"].is_string(), "{menu}");
    let route = format!("/api/v1/households/{home}/recipes/poulet-riz/nutrition");
    assert_eq!(alice.get(&route).await.status, StatusCode::OK);
}

#[sqlx::test]
async fn route_recette(db: PgPool) {
    let app = setup(&db).await;
    let (mut alice, home) = published(&app).await;
    let ok = alice
        .get(&format!(
            "/api/v1/households/{home}/recipes/poulet-riz/nutrition"
        ))
        .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    assert_eq!(ok.json["serve"][0]["label"], "Poulet");
    let unknown = alice
        .get(&format!(
            "/api/v1/households/{home}/recipes/inconnue/nutrition"
        ))
        .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);

    let mut bob = app.browser();
    bob.login("bob@example.org").await;
    let foreign = bob
        .get(&format!(
            "/api/v1/households/{home}/recipes/poulet-riz/nutrition"
        ))
        .await;
    assert_eq!(foreign.status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn liste_porte_la_note(db: PgPool) {
    let app = setup(&db).await;
    let (mut alice, home) = published(&app).await;
    let list = alice
        .get(&format!("/api/v1/households/{home}/recipes"))
        .await
        .json;
    let recipe = list
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["slug"] == "poulet-riz")
        .unwrap();
    assert!(recipe["grade"].is_string(), "{recipe}");
}

#[sqlx::test]
async fn agenda_porte_les_apports(db: PgPool) {
    let app = setup(&db).await;
    let (mut alice, home) = published(&app).await;
    let agenda = alice
        .get(&format!(
            "/api/v1/households/{home}/agenda?from=2026-10-18&days=7"
        ))
        .await;
    assert_eq!(agenda.status, StatusCode::OK, "{}", agenda.text);
    assert!(
        agenda.json["slots"][0]["nutrition"]["grade"].is_string(),
        "{}",
        agenda.json["slots"][0]
    );
}
