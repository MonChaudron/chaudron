//! Prix estimés : rattachement des ingrédients aux sources (Open Prices, INSEE), prix unitaires, et leur usage dans
//! les courses, le menu et les recettes.

mod common;

use chaudron_server::{config::SignupPolicy, prices};
use common::{Browser, TestApp};
use serde_json::Value;
use sqlx::PgPool;

async fn alice(app: &TestApp) -> Browser {
    let mut alice = app.browser();
    alice.login("alice@example.org").await;
    alice
}

async fn ingredient(browser: &mut Browser, code: &str) -> Value {
    let list = browser.get("/api/v1/ingredients").await.json;
    list.as_array()
        .unwrap()
        .iter()
        .find(|i| i["code"] == code)
        .unwrap_or_else(|| panic!("ingrédient {code}"))
        .clone()
}

async fn off_category(db: &PgPool, code: &str) -> Option<String> {
    sqlx::query_scalar(
        "SELECT l.off_category FROM ingredient_price_links l JOIN ingredients i ON i.id = l.ingredient_id
         WHERE i.code = $1 AND l.state = 'fresh'",
    )
    .bind(code)
    .fetch_optional(db)
    .await
    .unwrap()
    .flatten()
}

#[sqlx::test]
async fn amorcage_complete_sans_remplacer(db: PgPool) {
    sqlx::query(
        "INSERT INTO ingredient_price_links (ingredient_id, state, off_category)
         SELECT id, 'fresh', 'en:baby-carrots' FROM ingredients WHERE code = 'carrot'",
    )
    .execute(&db)
    .await
    .unwrap();

    let added = prices::seed_links(&db).await.unwrap();
    assert!(added > 0, "l'amorçage ajoute des rattachements");
    assert_eq!(
        off_category(&db, "carrot").await.as_deref(),
        Some("en:baby-carrots"),
        "un rattachement existant n'est jamais remplacé"
    );
    assert_eq!(off_category(&db, "leek").await.as_deref(), Some("en:leeks"));
    assert_eq!(
        prices::seed_links(&db).await.unwrap(),
        0,
        "second passage : rien à ajouter"
    );
}

#[sqlx::test]
async fn ingredient_expose_rattachement_et_prix(db: PgPool) {
    prices::seed_links(&db).await.unwrap();
    let app = TestApp::new(db, SignupPolicy::Open).await;
    let mut alice = alice(&app).await;

    let carrot = ingredient(&mut alice, "carrot").await;
    assert_eq!(carrot["price_links"][0]["state"], "fresh");
    assert_eq!(carrot["price_links"][0]["off_category"], "en:carrots");
    assert_eq!(carrot["prices"], Value::Array(Vec::new()));
}
