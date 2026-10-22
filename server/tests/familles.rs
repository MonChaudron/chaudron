//! Familles de recettes : les déclinaisons d'un même plat, une préférée par foyer. À la création ou à la proposition,
//! Chaudron signale les recettes proches ; l'auteur décide (à part, une autre déclinaison, la même en mieux), rien
//! n'est automatique.

mod common;

use axum::http::StatusCode;
use chaudron_server::config::SignupPolicy;
use common::{Browser, TestApp, config};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const ADMIN: &str = "admin@example.org";

async fn app(db: PgPool) -> TestApp {
    TestApp::with(db, |base| {
        let mut config = config(base, SignupPolicy::Open);
        config.admin_emails = vec![ADMIN.into()];
        config
    })
    .await
}

async fn home_of(app: &TestApp, email: &str, name: &str) -> (Browser, Uuid) {
    let mut browser = app.browser();
    browser.login(email).await;
    let home = browser.household(name).await;
    (browser, home)
}

fn recipe(title: &str, lines: &[(&str, &str, f64)], equipment: &[&str]) -> Value {
    json!({
        "title": title,
        "ingredients": lines.iter().map(|(code, role, qty)| json!({"ingredient": code, "qty": qty, "unit": "g", "role": role})).collect::<Vec<_>>(),
        "variants": [{"label": "Four", "equipment": equipment, "active_min": 20, "total_min": 60,
                      "phases": [{"title": "Cuire", "text": "Cuire."}, {"title": "Servir", "text": "Servir."}]}],
    })
}

fn dauphinois() -> Value {
    recipe(
        "Gratin dauphinois",
        &[
            ("potato", "starch", 250.0),
            ("milk", "other", 100.0),
            ("garlic", "other", 2.0),
            ("cream", "fat", 30.0),
        ],
        &["oven"],
    )
}

fn a_la_creme() -> Value {
    recipe(
        "Gratin de pommes de terre à la crème",
        &[
            ("potato", "starch", 250.0),
            ("cream", "fat", 80.0),
            ("garlic", "other", 2.0),
        ],
        &["oven"],
    )
}

fn chili() -> Value {
    recipe(
        "Chili con carne",
        &[
            ("minced_beef", "protein", 150.0),
            ("rice", "starch", 50.0),
            ("tomato", "veg", 80.0),
            ("onion", "veg", 40.0),
        ],
        &["hob"],
    )
}

#[sqlx::test]
async fn recettes_proches_a_la_creation(db: PgPool) {
    let app = app(db).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    for (slug, body) in [("gratin-dauphinois", dauphinois()), ("chili", chili())] {
        alice
            .put(&format!("/api/v1/households/{home}/recipes/{slug}"), body)
            .await;
    }
    let similar = alice
        .post(
            &format!("/api/v1/households/{home}/recipes/similar"),
            a_la_creme(),
        )
        .await;
    assert_eq!(similar.status, StatusCode::OK, "{}", similar.text);
    let list = similar.json.as_array().unwrap();
    assert_eq!(list.len(), 1, "{}", similar.text);
    assert_eq!(list[0]["slug"], "gratin-dauphinois");
    assert!(list[0]["score"].as_i64().unwrap() >= 50);
    assert!(
        list[0]["reasons"][0]
            .as_str()
            .unwrap()
            .contains("Pomme de terre")
    );

    // Rien d'automatique : publiée telle quelle, la recette reste à part.
    alice
        .put(
            &format!("/api/v1/households/{home}/recipes/gratin-creme"),
            a_la_creme(),
        )
        .await;
    let read = alice
        .get(&format!("/api/v1/households/{home}/recipes/gratin-creme"))
        .await
        .json;
    assert_eq!(read["family"], Value::Null);
    // Une recette ne se ressemble pas à elle-même.
    let mut again = dauphinois();
    again["slug"] = json!("gratin-dauphinois");
    let itself = alice
        .post(&format!("/api/v1/households/{home}/recipes/similar"), again)
        .await;
    assert!(
        itself
            .json
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["slug"] != "gratin-dauphinois")
    );
}

#[sqlx::test]
async fn famille_et_preferee(db: PgPool) {
    let app = app(db).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    for (slug, body) in [
        ("gratin-dauphinois", dauphinois()),
        ("gratin-creme", a_la_creme()),
    ] {
        alice
            .put(&format!("/api/v1/households/{home}/recipes/{slug}"), body)
            .await;
    }
    let joined = alice
        .put(
            &format!("/api/v1/households/{home}/recipes/gratin-creme/family"),
            json!({"with": "gratin-dauphinois"}),
        )
        .await;
    assert_eq!(joined.status, StatusCode::OK, "{}", joined.text);
    let family = &joined.json["family"];
    assert_eq!(family["title"], "Gratin dauphinois");
    assert_eq!(family["members"].as_array().unwrap().len(), 2);
    assert_eq!(family["preferred"], Value::Null);
    let family_id = family["id"].as_str().unwrap().to_owned();

    let preferred = alice
        .put(
            &format!("/api/v1/households/{home}/families/{family_id}/preferred"),
            json!({"slug": "gratin-creme"}),
        )
        .await;
    assert_eq!(
        preferred.status,
        StatusCode::NO_CONTENT,
        "{}",
        preferred.text
    );
    let read = alice
        .get(&format!(
            "/api/v1/households/{home}/recipes/gratin-dauphinois"
        ))
        .await
        .json;
    assert_eq!(read["family"]["preferred"], "gratin-creme");
    let list = alice
        .get(&format!("/api/v1/households/{home}/recipes"))
        .await
        .json;
    let creme = list
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["slug"] == "gratin-creme")
        .unwrap();
    assert_eq!(creme["family_id"], family_id.as_str());
    assert_eq!(creme["preferred"], true);

    // Bob ne voit pas la famille d'Alice ; sa préférée est la sienne.
    let (mut bob, bob_home) = home_of(&app, "bob@example.org", "Chez Bob").await;
    assert_eq!(
        bob.put(
            &format!("/api/v1/households/{bob_home}/families/{family_id}/preferred"),
            json!({"slug": "gratin-creme"}),
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // En sortir.
    let left = alice
        .delete(&format!(
            "/api/v1/households/{home}/recipes/gratin-creme/family"
        ))
        .await;
    assert_eq!(left.status, StatusCode::NO_CONTENT);
    let read = alice
        .get(&format!("/api/v1/households/{home}/recipes/gratin-creme"))
        .await
        .json;
    assert_eq!(read["family"], Value::Null);
}

#[sqlx::test]
async fn la_meme_en_mieux_devient_des_propositions(db: PgPool) {
    let app = app(db).await;
    let (mut admin, admin_home) = home_of(&app, ADMIN, "Chez l'admin").await;
    admin
        .put(
            &format!("/api/v1/households/{admin_home}/recipes/gratin-dauphinois"),
            dauphinois(),
        )
        .await;
    let id = admin
        .post(
            &format!("/api/v1/households/{admin_home}/recipes/gratin-dauphinois/submit"),
            json!({}),
        )
        .await
        .json["id"]
        .as_str()
        .unwrap()
        .to_owned();
    admin
        .post(
            &format!("/api/v1/community/submissions/{id}/accept"),
            json!({}),
        )
        .await;

    // Alice écrit « son » gratin dauphinois, le même en mieux : il devient sa copie, et ses différences des
    // propositions sur celui de la communauté.
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    let mut better = dauphinois();
    better["ingredients"][1]["qty"] = json!(120);
    better["variants"][0]["phases"][0]["text"] =
        json!("Cuire 1 h à 160 °C, couvert les 30 premières minutes.");
    let copy = alice
        .post(
            &format!("/api/v1/households/{home}/recipes/gratin-dauphinois/copy"),
            json!({"slug": "mon-gratin", "recipe": better}),
        )
        .await;
    assert_eq!(copy.status, StatusCode::CREATED, "{}", copy.text);
    assert_eq!(
        copy.json["variants"][0]["phases"][0]["text"],
        "Cuire 1 h à 160 °C, couvert les 30 premières minutes."
    );
    let proposed = alice
        .post(
            &format!("/api/v1/households/{home}/recipes/mon-gratin/propose-changes"),
            json!({}),
        )
        .await;
    assert_eq!(proposed.status, StatusCode::CREATED, "{}", proposed.text);
    let kinds: Vec<&str> = proposed
        .json
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, vec!["proposal", "proposal"]);
}

#[sqlx::test]
async fn proposer_comme_declinaison_et_import(db: PgPool) {
    let app = app(db).await;
    let (mut admin, admin_home) = home_of(&app, ADMIN, "Chez l'admin").await;
    for (slug, body) in [
        ("gratin-dauphinois", dauphinois()),
        ("gratin-creme", a_la_creme()),
    ] {
        admin
            .put(
                &format!("/api/v1/households/{admin_home}/recipes/{slug}"),
                body,
            )
            .await;
        let id = admin
            .post(&format!("/api/v1/households/{admin_home}/recipes/{slug}/submit"), json!({
                "family_with": if slug == "gratin-creme" { json!("gratin-dauphinois") } else { Value::Null },
            }))
            .await
            .json["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let accepted = admin
            .post(
                &format!("/api/v1/community/submissions/{id}/accept"),
                json!({}),
            )
            .await;
        assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.text);
    }
    // Acceptée comme déclinaison : la famille est faite.
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    let read = alice
        .get(&format!("/api/v1/households/{home}/recipes/gratin-creme"))
        .await
        .json;
    assert_eq!(read["family"]["members"].as_array().unwrap().len(), 2);
    // Un foyer ne range pas une recette de la communauté dans une famille ; l'admin, si.
    assert_eq!(
        alice
            .delete(&format!(
                "/api/v1/households/{home}/recipes/gratin-creme/family"
            ))
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    // L'import signale les recettes proches, sans rien en faire.
    let token = alice
        .post(
            &format!("/api/v1/households/{home}/tokens"),
            json!({"name": "Claude", "scopes": ["recipes:write"]}),
        )
        .await
        .json["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut lot_recipe = a_la_creme();
    lot_recipe["slug"] = json!("gratin-du-dimanche");
    let imported = app
        .api_client(&token)
        .post(
            &format!("/api/v1/households/{home}/import"),
            json!({"recipes": [lot_recipe]}),
        )
        .await;
    assert_eq!(imported.status, StatusCode::OK, "{}", imported.text);
    assert_eq!(imported.json["similar"][0]["slug"], "gratin-du-dimanche");
    assert_eq!(imported.json["similar"][0]["similar_to"][0], "gratin-creme");
}
