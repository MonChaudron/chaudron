//! Prix estimés : rattachement des ingrédients aux sources (Open Prices, INSEE), prix unitaires, et leur usage dans
//! les courses, le menu et les recettes.

mod common;

use chaudron_server::{AppState, config::SignupPolicy, prices};
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

// Sources et mise à jour mensuelle

async fn sources(db: &PgPool) -> (AppState, common::Fake) {
    prices::seed_links(db).await.unwrap();
    let fake = common::Fake::start().await;
    let state = AppState::new(db.clone(), common::config(&fake.base, SignupPolicy::Closed));
    (state, fake)
}

async fn stored(db: &PgPool, code: &str) -> Option<(i32, String, String, i32)> {
    sqlx::query_as(
        "SELECT p.cents, p.per, p.source, p.observations FROM ingredient_prices p
         JOIN ingredients i ON i.id = p.ingredient_id WHERE i.code = $1",
    )
    .bind(code)
    .fetch_optional(db)
    .await
    .unwrap()
}

async fn last_run_ok(db: &PgPool) -> bool {
    sqlx::query_scalar("SELECT ok FROM price_runs ORDER BY started_at DESC LIMIT 1")
        .fetch_one(db)
        .await
        .unwrap()
}

fn carrots(fake: &common::Fake) {
    for (euros, days) in [(2.29, 15), (2.45, 13), (2.62, 11), (2.80, 9), (2.84, 7)] {
        fake.prices.raw("en:carrots", euros, days, "France", &[]);
    }
    fake.prices.raw("en:carrots", 1.10, 5, "Deutschland", &[]);
    fake.prices.series("000641422", &[("2026-09", 2.55)]);
}

#[sqlx::test]
async fn passage_ecrit_les_prix(db: PgPool) {
    let (state, fake) = sources(&db).await;
    carrots(&fake);

    assert!(prices::run::check(&state).await, "premier passage");
    assert_eq!(
        stored(&db, "carrot").await,
        Some((262, "kg".into(), "open_prices".into(), 5)),
        "médiane des cinq relevés de France"
    );
    assert!(last_run_ok(&db).await);
    assert!(!prices::run::check(&state).await, "pas avant 30 jours");
}

#[sqlx::test]
async fn conditionne_ramene_au_kilo(db: PgPool) {
    sqlx::query(
        "INSERT INTO ingredients (id, code, name, search_key, aisle, default_unit, default_state)
         VALUES ($1, 'gnocchi', 'Gnocchi', 'gnocchi', 'fresh', 'g', 'fresh')",
    )
    .bind(uuid::Uuid::now_v7())
    .execute(&db)
    .await
    .unwrap();
    let (state, fake) = sources(&db).await;
    for days in [20, 18, 16, 14, 12] {
        fake.prices.product("en:gnocchi", 2.45, 300.0, days);
    }

    prices::run::check(&state).await;
    assert_eq!(
        stored(&db, "gnocchi").await,
        Some((817, "kg".into(), "open_prices".into(), 5)),
        "2,45 € les 300 g"
    );
}

#[sqlx::test]
async fn source_injoignable_garde_les_prix(db: PgPool) {
    let (state, fake) = sources(&db).await;
    carrots(&fake);
    prices::run::check(&state).await;

    sqlx::query("UPDATE price_runs SET started_at = now() - interval '31 days'")
        .execute(&db)
        .await
        .unwrap();
    fake.prices.set_down(true);
    assert!(
        prices::run::check(&state).await,
        "un mois plus tard, le passage a lieu"
    );
    assert!(!last_run_ok(&db).await, "en échec");
    assert_eq!(
        stored(&db, "carrot").await.map(|p| p.0),
        Some(262),
        "le prix reste"
    );

    fake.prices.set_down(false);
    assert!(
        prices::run::check(&state).await,
        "un passage en échec ne repousse pas le suivant de 30 jours"
    );
    assert!(last_run_ok(&db).await);
}
