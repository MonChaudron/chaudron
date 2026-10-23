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

// Rattachements dans le lot

async fn home(app: &TestApp) -> (Browser, String) {
    let mut alice = alice(app).await;
    let home = alice.household("Maison").await.to_string();
    (alice, home)
}

fn brussels_lot(carrot_category: &str) -> Value {
    serde_json::json!({"ingredients": [
        {"name": "Carotte", "aisle": "produce", "default_unit": "g", "default_state": "fresh",
         "price_links": [{"state": "fresh", "off_category": carrot_category}]},
        {"name": "Chou de Bruxelles", "aisle": "produce", "default_unit": "g", "default_state": "fresh",
         "piece_g": 12, "price_links": [{"state": "fresh", "off_category": "en:brussels-sprouts"}]},
    ]})
}

#[sqlx::test]
async fn lot_complete_sans_remplacer(db: PgPool) {
    prices::seed_links(&db).await.unwrap();
    let app = TestApp::new(db.clone(), SignupPolicy::Open).await;
    let (mut alice, home) = home(&app).await;

    let response = alice
        .post(
            &format!("/api/v1/households/{home}/import"),
            brussels_lot("en:organic-carrots"),
        )
        .await;
    assert_eq!(
        response.status,
        axum::http::StatusCode::OK,
        "{}",
        response.text
    );
    assert_eq!(
        off_category(&db, "carrot").await.as_deref(),
        Some("en:carrots"),
        "rien n'est remplacé"
    );
    assert_eq!(
        off_category(&db, "chou_de_bruxelles").await.as_deref(),
        Some("en:brussels-sprouts")
    );
    let sprouts = ingredient(&mut alice, "chou_de_bruxelles").await;
    assert_eq!(sprouts["piece_g"], 12.0);

    // Un ingrédient connu sans rattachement pour cette forme le reçoit du lot.
    sqlx::query("DELETE FROM ingredient_price_links WHERE ingredient_id = (SELECT id FROM ingredients WHERE code = 'carrot')")
        .execute(&db)
        .await
        .unwrap();
    alice
        .post(
            &format!("/api/v1/households/{home}/import"),
            brussels_lot("en:organic-carrots"),
        )
        .await;
    assert_eq!(
        off_category(&db, "carrot").await.as_deref(),
        Some("en:organic-carrots")
    );
}

async fn priced(browser: &mut Browser, code: &str) -> Value {
    ingredient(browser, code).await["prices"].clone()
}

#[sqlx::test]
async fn ingredient_nouveau_chiffre_tout_de_suite(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Open).await;
    for (euros, days) in [(3.96, 20), (3.80, 18), (4.10, 16), (3.70, 14), (4.00, 12)] {
        app.fake
            .prices
            .raw("en:brussels-sprouts", euros, days, "France", &[]);
    }
    let (mut alice, home) = home(&app).await;

    let response = alice
        .post(
            &format!("/api/v1/households/{home}/import"),
            brussels_lot("en:carrots"),
        )
        .await;
    assert_eq!(
        response.status,
        axum::http::StatusCode::OK,
        "{}",
        response.text
    );
    let mut prices = Value::Null;
    for _ in 0..50 {
        prices = priced(&mut alice, "chou_de_bruxelles").await;
        if prices.as_array().is_some_and(|p| !p.is_empty()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(prices[0]["cents"], 396, "{prices}");
    assert_eq!(prices[0]["per"], "kg");
}

#[sqlx::test]
async fn import_reussit_source_coupee(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Open).await;
    app.fake.prices.set_down(true);
    let (mut alice, home) = home(&app).await;

    let response = alice
        .post(
            &format!("/api/v1/households/{home}/import"),
            brussels_lot("en:carrots"),
        )
        .await;
    assert_eq!(
        response.status,
        axum::http::StatusCode::OK,
        "{}",
        response.text
    );
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(
        priced(&mut alice, "chou_de_bruxelles").await,
        Value::Array(Vec::new())
    );
}

// Courses chiffrées

/// Deux adultes et deux ados (à la maison le mercredi et le samedi midi).
async fn family(app: &TestApp) -> (Browser, String) {
    let (mut alice, home) = home(app).await;
    let members = format!("/api/v1/households/{home}/members");
    for (name, kind) in [
        ("Alice", "adult"),
        ("Bob", "adult"),
        ("Léo", "teen"),
        ("Zoé", "teen"),
    ] {
        let mut member = alice
            .post(&members, serde_json::json!({"name": name, "kind": kind}))
            .await
            .json;
        if kind == "teen" {
            member["presence"]["lunch"] =
                serde_json::json!([false, false, true, false, false, true, false]);
            let url = format!("{members}/{}", member["id"].as_str().unwrap());
            alice.put(&url, member).await;
        }
    }
    (alice, home)
}

async fn set_price(db: &PgPool, code: &str, state: &str, cents: i32, per: &str) {
    sqlx::query(
        "INSERT INTO ingredient_prices (ingredient_id, state, cents, per, source, observations, observed_on)
         SELECT id, $2, $3, $4, 'open_prices', 5, current_date FROM ingredients WHERE code = $1",
    )
    .bind(code)
    .bind(state)
    .bind(cents)
    .bind(per)
    .execute(db)
    .await
    .unwrap();
}

async fn recipe(alice: &mut Browser, home: &str, slug: &str, title: &str, lines: Value) {
    let body = serde_json::json!({
        "title": title, "ingredients": lines,
        "variants": [{"label": "Cocotte", "active_min": 10, "total_min": 30,
                      "phases": [{"title": "Cuire", "text": "Cuire."}]}],
    });
    let response = alice
        .put(&format!("/api/v1/households/{home}/recipes/{slug}"), body)
        .await;
    assert!(response.status.is_success(), "{}", response.text);
}

/// Chili (bœuf 10 €/kg, riz 2 €/kg) dimanche, avec les boîtes du lundi ; saumon (20 €/kg, épinards surgelés
/// 5 €/kg) mercredi, sans boîtes.
async fn priced_week(db: &PgPool, app: &TestApp) -> (Browser, String) {
    let (mut alice, home) = family(app).await;
    set_price(db, "minced_beef", "fresh", 1000, "kg").await;
    set_price(db, "rice", "dried", 200, "kg").await;
    set_price(db, "salmon", "fresh", 2000, "kg").await;
    set_price(db, "spinach", "frozen", 500, "kg").await;
    recipe(
        &mut alice,
        &home,
        "chili",
        "Chili",
        serde_json::json!([
            {"ingredient": "minced_beef", "qty": 150, "unit": "g", "role": "protein"},
            {"ingredient": "rice", "qty": 50, "unit": "g", "role": "starch"},
            {"ingredient": "coriander", "unit": "g", "role": "other", "optional": true},
        ]),
    )
    .await;
    recipe(
        &mut alice,
        &home,
        "saumon",
        "Saumon",
        serde_json::json!([
            {"ingredient": "salmon", "qty": 150, "unit": "g", "role": "protein"},
            {"ingredient": "spinach", "state": "frozen", "qty": 100, "unit": "g", "role": "veg"},
        ]),
    )
    .await;
    let response = alice
        .put(
            &format!("/api/v1/households/{home}/menus/2026-10-18"),
            serde_json::json!({"slots": [
                {"date": "2026-10-18", "meal": "dinner", "recipe": "chili", "covers_next_lunch": true},
                {"date": "2026-10-21", "meal": "dinner", "recipe": "saumon", "covers_next_lunch": false},
            ]}),
        )
        .await;
    assert!(response.status.is_success(), "{}", response.text);
    (alice, home)
}

async fn shopping(alice: &mut Browser, home: &str) -> Value {
    let response = alice
        .get(&format!(
            "/api/v1/households/{home}/menus/2026-10-18/shopping"
        ))
        .await;
    assert!(response.status.is_success(), "{}", response.text);
    response.json
}

fn line<'a>(list: &'a Value, label: &str) -> &'a Value {
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["label"] == label)
        .unwrap_or_else(|| panic!("{label} absent de {list}"))
}

#[sqlx::test]
async fn courses_portent_leur_prix(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Open).await;
    let (mut alice, home) = priced_week(&db, &app).await;
    let list = shopping(&mut alice, &home).await;

    // Dimanche : 4 au dîner et 2 boîtes (les adultes) ; protéine × 6, féculent × 8 (les ados en double).
    assert_eq!(line(&list, "Bœuf haché")["price_cents"], 900);
    assert_eq!(line(&list, "Riz")["price_cents"], 80);
    // Mercredi : 4 au dîner.
    assert_eq!(line(&list, "Saumon")["price_cents"], 1200);
    assert_eq!(line(&list, "Épinard")["price_cents"], 200);
    let price = &list["price"];
    assert_eq!(price["total_cents"], 2380);
    assert_eq!(price["portions"], 10);
    assert_eq!(price["per_portion_cents"], 238);
    assert_eq!(price["shared_cents"], 0);
    assert_eq!(price["missing"], 0);
    assert_eq!(price["meals"][0]["recipe"], "chili");
    assert_eq!(price["meals"][0]["cents"], 980);
    assert_eq!(price["meals"][0]["complete"], true);
    assert_eq!(price["meals"][1]["cents"], 1400);
    assert_eq!(price["over_budget"], Value::Null);
}

#[sqlx::test]
async fn placard_au_prorata(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Open).await;
    let (mut alice, home) = priced_week(&db, &app).await;
    sqlx::query("UPDATE ingredients SET density = 0.92 WHERE code = 'olive'")
        .execute(&db)
        .await
        .unwrap();
    set_price(&db, "olive", "canned", 900, "l").await;
    recipe(&mut alice, &home, "saumon", "Saumon", serde_json::json!([
        {"ingredient": "salmon", "qty": 150, "unit": "g", "role": "protein"},
        {"ingredient": "spinach", "state": "frozen", "qty": 100, "unit": "g", "role": "veg"},
        {"ingredient": "olive", "state": "canned", "qty": 1, "unit": "tbsp", "role": "fat", "per_batch": true},
    ]))
    .await;

    let list = shopping(&mut alice, &home).await;
    let oil = line(&list, "Olive");
    assert_eq!(oil["pantry"], true);
    assert_eq!(
        oil["price_cents"], 14,
        "une cuillère à 9 €/l, pas la bouteille"
    );
    assert_eq!(list["price"]["total_cents"], 2394);
}

#[sqlx::test]
async fn articles_sans_prix_comptes(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Open).await;
    let (mut alice, home) = priced_week(&db, &app).await;
    recipe(
        &mut alice,
        &home,
        "saumon",
        "Saumon",
        serde_json::json!([
            {"ingredient": "salmon", "qty": 150, "unit": "g", "role": "protein"},
            {"ingredient": "spinach", "state": "frozen", "qty": 100, "unit": "g", "role": "veg"},
            {"ingredient": "garlic", "qty": 2, "unit": "piece", "role": "other", "per_batch": true},
        ]),
    )
    .await;
    let response = alice
        .post(
            &format!("/api/v1/households/{home}/shopping/ops"),
            serde_json::json!({"ops": [{"op_id": uuid::Uuid::now_v7(), "at": "2026-10-17T10:00:00Z",
                "week_start": "2026-10-18", "kind": "add", "key": "manual:pain", "label": "Pain",
                "block": "special"}]}),
        )
        .await;
    assert!(response.status.is_success(), "{}", response.text);

    let list = shopping(&mut alice, &home).await;
    assert_eq!(line(&list, "Ail")["price_cents"], Value::Null);
    assert_eq!(line(&list, "Pain")["price_cents"], Value::Null);
    assert_eq!(list["price"]["missing"], 1, "l'ail, pas l'ajout à la main");
    assert_eq!(list["price"]["total_cents"], 2380);
    assert_eq!(list["price"]["meals"][1]["complete"], false);
}

#[sqlx::test]
async fn menu_vide_sans_recap(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Open).await;
    let (mut alice, home) = family(&app).await;
    let response = alice
        .put(
            &format!("/api/v1/households/{home}/menus/2026-10-18"),
            serde_json::json!({"slots": []}),
        )
        .await;
    assert!(response.status.is_success(), "{}", response.text);
    assert_eq!(shopping(&mut alice, &home).await["price"], Value::Null);
}

#[sqlx::test]
async fn remplacement_suit_le_prix(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Open).await;
    let (mut alice, home) = priced_week(&db, &app).await;
    set_price(&db, "cod", "fresh", 1000, "kg").await;
    let substitution = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO substitutions (id, ingredient_id, substitute_id, factor, source)
         SELECT $1, s.id, c.id, 1, 'admin' FROM ingredients s, ingredients c WHERE s.code = 'salmon' AND c.code = 'cod'",
    )
    .bind(substitution)
    .execute(&db)
    .await
    .unwrap();
    let list = shopping(&mut alice, &home).await;
    let salmon = line(&list, "Saumon").clone();
    assert_eq!(salmon["substitutes"][0]["price_cents"], 600);

    let response = alice
        .post(
            &format!("/api/v1/households/{home}/shopping/ops"),
            serde_json::json!({"ops": [{"op_id": uuid::Uuid::now_v7(), "at": "2026-10-17T10:00:00Z",
                "week_start": "2026-10-18", "kind": "substitute", "key": salmon["key"],
                "substitution_id": substitution}]}),
        )
        .await;
    assert!(response.status.is_success(), "{}", response.text);
    let list = shopping(&mut alice, &home).await;
    assert_eq!(line(&list, "Cabillaud")["price_cents"], 600);
    assert_eq!(
        line(&list, "Cabillaud")["replaces"]["price_cents"],
        1200,
        "le prix d'origine, pour revenir hors ligne"
    );
    assert_eq!(list["price"]["total_cents"], 1780);
}

// Repas et recettes chiffrés, tiers

async fn recipes_list(alice: &mut Browser, home: &str, query: &str) -> Value {
    let response = alice
        .get(&format!("/api/v1/households/{home}/recipes{query}"))
        .await;
    assert!(response.status.is_success(), "{}", response.text);
    response.json
}

fn recipe_price<'a>(list: &'a Value, slug: &str) -> &'a Value {
    &list
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["slug"] == slug)
        .unwrap_or_else(|| panic!("{slug} absent de {list}"))["price"]
}

/// Six recettes d'une ligne de bœuf (10 €/kg), de 100 à 600 g par portion ; les deux dernières à la communauté.
async fn six_recipes(db: &PgPool, app: &TestApp) -> (Browser, String) {
    let (mut alice, home) = home(app).await;
    set_price(db, "minced_beef", "fresh", 1000, "kg").await;
    for grams in [100, 200, 300, 400, 500, 600] {
        recipe(
            &mut alice,
            &home,
            &format!("boeuf-{grams}"),
            &format!("Bœuf {grams}"),
            serde_json::json!([
                {"ingredient": "minced_beef", "qty": grams, "unit": "g", "role": "protein"},
            ]),
        )
        .await;
    }
    sqlx::query("UPDATE recipes SET household_id = NULL WHERE slug IN ('boeuf-500', 'boeuf-600')")
        .execute(db)
        .await
        .unwrap();
    (alice, home)
}

#[sqlx::test]
async fn tiers_sur_foyer_et_communaute(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Open).await;
    let (mut alice, home) = six_recipes(&db, &app).await;
    let list = recipes_list(&mut alice, &home, "").await;
    for (slug, tier, cents) in [
        ("boeuf-100", "low", 100),
        ("boeuf-200", "low", 200),
        ("boeuf-300", "mid", 300),
        ("boeuf-400", "mid", 400),
        ("boeuf-500", "high", 500),
        ("boeuf-600", "high", 600),
    ] {
        let price = recipe_price(&list, slug);
        assert_eq!(price["tier"], tier, "{slug} : {price}");
        assert_eq!(price["per_portion_cents"], cents, "{slug}");
        assert_eq!(price["complete"], true);
    }
}

#[sqlx::test]
async fn recette_sans_prix_hors_des_tiers(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Open).await;
    let (mut alice, home) = six_recipes(&db, &app).await;
    recipe(
        &mut alice,
        &home,
        "boeuf-ail",
        "Bœuf à l'ail",
        serde_json::json!([
            {"ingredient": "minced_beef", "qty": 50, "unit": "g", "role": "protein"},
            {"ingredient": "garlic", "qty": 2, "unit": "piece", "role": "other", "per_batch": true},
        ]),
    )
    .await;
    let list = recipes_list(&mut alice, &home, "").await;
    let garlic = recipe_price(&list, "boeuf-ail");
    assert_eq!(garlic["complete"], false);
    assert_eq!(garlic["tier"], Value::Null);
    assert_eq!(
        recipe_price(&list, "boeuf-100")["tier"],
        "low",
        "les tiers ne comptent pas la recette incomplète"
    );
    assert_eq!(recipe_price(&list, "boeuf-300")["tier"], "mid");
}

#[sqlx::test]
async fn prix_au_choix_pour_la_tablee(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Open).await;
    let (mut alice, home) = family(&app).await;
    set_price(&db, "minced_beef", "fresh", 1000, "kg").await;
    set_price(&db, "rice", "dried", 200, "kg").await;
    recipe(
        &mut alice,
        &home,
        "chili",
        "Chili",
        serde_json::json!([
            {"ingredient": "minced_beef", "qty": 150, "unit": "g", "role": "protein"},
            {"ingredient": "rice", "qty": 50, "unit": "g", "role": "starch"},
        ]),
    )
    .await;

    // Mardi soir : 4 au dîner, 4 boîtes le mercredi midi (les ados déjeunent à la maison).
    let list = recipes_list(
        &mut alice,
        &home,
        "?price_date=2026-10-20&price_meal=dinner",
    )
    .await;
    let price = recipe_price(&list, "chili");
    assert_eq!(price["cents"], 1320, "{price}");
    assert_eq!(price["per_portion_cents"], 160);
    let list = recipes_list(&mut alice, &home, "").await;
    assert_eq!(
        recipe_price(&list, "chili")["cents"],
        160,
        "sans repas : la portion standard"
    );
}

#[sqlx::test]
async fn menu_chiffre_par_creneau(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Open).await;
    let (mut alice, home) = priced_week(&db, &app).await;
    let response = alice
        .get(&format!("/api/v1/households/{home}/menus/2026-10-18"))
        .await;
    assert!(response.status.is_success(), "{}", response.text);
    let menu = response.json;
    assert_eq!(menu["slots"][0]["price"]["cents"], 980);
    assert_eq!(menu["slots"][0]["price"]["complete"], true);
    assert_eq!(menu["slots"][1]["price"]["cents"], 1400);
    assert_eq!(
        menu["price"]["total_cents"], 2380,
        "le même total que les courses"
    );
}

// Plafond du foyer

async fn set_budget(alice: &mut Browser, home: &str, cents: Value) -> common::Response {
    alice
        .patch(
            &format!("/api/v1/households/{home}"),
            serde_json::json!({"weekly_budget_cents": cents}),
        )
        .await
}

#[sqlx::test]
async fn plafond_reserve_au_proprietaire(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Open).await;
    let (mut alice, home) = home(&app).await;
    let response = set_budget(&mut alice, &home, serde_json::json!(14000)).await;
    assert_eq!(
        response.status,
        axum::http::StatusCode::OK,
        "{}",
        response.text
    );
    assert_eq!(response.json["weekly_budget_cents"], 14000);

    let token = alice.invite(home.parse().unwrap()).await;
    let mut bob = app.browser();
    bob.login_with("bob@example.org", Some(&token)).await;
    let response = set_budget(&mut bob, &home, serde_json::json!(50000)).await;
    assert_eq!(
        response.status,
        axum::http::StatusCode::FORBIDDEN,
        "{}",
        response.text
    );

    let response = alice
        .patch(
            &format!("/api/v1/households/{home}"),
            serde_json::json!({"name": "La maison"}),
        )
        .await;
    assert_eq!(
        response.json["weekly_budget_cents"], 14000,
        "un autre champ ne touche pas au plafond"
    );
    let response = set_budget(&mut alice, &home, Value::Null).await;
    assert_eq!(
        response.json["weekly_budget_cents"],
        Value::Null,
        "plafond retiré"
    );
}

fn budget_warnings(menu: &Value) -> usize {
    menu["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|w| w["kind"] == "budget")
        .count()
}

#[sqlx::test]
async fn menu_au_dessus_du_plafond_averti(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Open).await;
    let (mut alice, home) = priced_week(&db, &app).await;
    set_budget(&mut alice, &home, serde_json::json!(2000)).await;

    let menu = alice
        .get(&format!("/api/v1/households/{home}/menus/2026-10-18"))
        .await
        .json;
    assert_eq!(menu["price"]["budget_cents"], 2000);
    assert_eq!(menu["price"]["over_budget"], true);
    assert_eq!(budget_warnings(&menu), 1, "{}", menu["warnings"]);
    assert_eq!(
        shopping(&mut alice, &home).await["price"]["over_budget"],
        true
    );

    set_budget(&mut alice, &home, serde_json::json!(3000)).await;
    let menu = alice
        .get(&format!("/api/v1/households/{home}/menus/2026-10-18"))
        .await
        .json;
    assert_eq!(menu["price"]["over_budget"], false);
    assert_eq!(budget_warnings(&menu), 0);
}

#[sqlx::test]
async fn plafond_incomplet_sans_avertissement(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Open).await;
    let (mut alice, home) = priced_week(&db, &app).await;
    recipe(
        &mut alice,
        &home,
        "saumon",
        "Saumon",
        serde_json::json!([
            {"ingredient": "salmon", "qty": 150, "unit": "g", "role": "protein"},
            {"ingredient": "garlic", "qty": 2, "unit": "piece", "role": "other", "per_batch": true},
        ]),
    )
    .await;
    set_budget(&mut alice, &home, serde_json::json!(1000)).await;

    let menu = alice
        .get(&format!("/api/v1/households/{home}/menus/2026-10-18"))
        .await
        .json;
    assert_eq!(menu["price"]["missing"], 1);
    assert_eq!(
        menu["price"]["over_budget"],
        Value::Null,
        "comparaison incomplète"
    );
    assert_eq!(budget_warnings(&menu), 0);
}

// Estimation à blanc

/// Un lot : le chili (bœuf et riz du référentiel) dimanche avec les boîtes, et un plat de choux de Bruxelles,
/// ingrédient nouveau rattaché à Open Prices, mercredi.
fn week_lot() -> Value {
    let phases = serde_json::json!([{"title": "Cuire", "text": "Cuire."}]);
    serde_json::json!({
        "ingredients": [
            {"name": "Chou de Bruxelles", "aisle": "produce", "default_unit": "g", "default_state": "fresh",
             "price_links": [{"state": "fresh", "off_category": "en:brussels-sprouts"}]},
        ],
        "recipes": [
            {"slug": "chili", "title": "Chili", "ingredients": [
                {"ingredient": "minced_beef", "qty": 150, "unit": "g", "role": "protein"},
                {"ingredient": "rice", "qty": 0.05, "unit": "kg", "role": "starch"},
             ], "variants": [{"label": "Cocotte", "active_min": 10, "total_min": 30, "phases": phases}]},
            {"slug": "choux", "title": "Choux rôtis", "ingredients": [
                {"ingredient": "chou_de_bruxelles", "qty": 200, "unit": "g", "role": "veg"},
                {"ingredient": "salmon", "qty": 150, "unit": "g", "role": "protein"},
             ], "variants": [{"label": "Four", "active_min": 10, "total_min": 30, "phases": phases}]},
        ],
        "menu": {"week_start": "2026-10-18", "slots": [
            {"date": "2026-10-18", "meal": "dinner", "recipe": "chili", "covers_next_lunch": true},
            {"date": "2026-10-21", "meal": "dinner", "recipe": "choux", "covers_next_lunch": false},
        ]},
    })
}

async fn priced_sources(db: &PgPool, app: &TestApp) {
    set_price(db, "minced_beef", "fresh", 1000, "kg").await;
    set_price(db, "rice", "dried", 200, "kg").await;
    set_price(db, "salmon", "fresh", 2000, "kg").await;
    for days in [20, 18, 16, 14, 12] {
        app.fake
            .prices
            .raw("en:brussels-sprouts", 4.00, days, "France", &[]);
    }
}

#[sqlx::test]
async fn estimation_egale_a_la_publication(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Open).await;
    priced_sources(&db, &app).await;
    let (mut alice, home) = family(&app).await;

    let response = alice
        .post(&format!("/api/v1/households/{home}/estimate"), week_lot())
        .await;
    assert_eq!(
        response.status,
        axum::http::StatusCode::OK,
        "{}",
        response.text
    );
    let estimate = response.json;
    // Mercredi, 4 au dîner : 800 g de choux à 4 €/kg et 600 g de saumon à 20 €/kg.
    assert_eq!(estimate["meals"][1]["cents"], 1520, "{estimate}");
    assert_eq!(estimate["meals"][0]["cents"], 980);
    assert_eq!(estimate["week"]["total_cents"], 2500);
    assert_eq!(estimate["unknown"], Value::Array(Vec::new()));

    let response = alice
        .post(&format!("/api/v1/households/{home}/import"), week_lot())
        .await;
    assert_eq!(
        response.status,
        axum::http::StatusCode::OK,
        "{}",
        response.text
    );
    for _ in 0..50 {
        if priced(&mut alice, "chou_de_bruxelles")
            .await
            .as_array()
            .is_some_and(|p| !p.is_empty())
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let list = shopping(&mut alice, &home).await;
    assert_eq!(
        list["price"]["total_cents"],
        estimate["week"]["total_cents"]
    );
    assert_eq!(list["price"]["meals"], estimate["meals"]);
}

#[sqlx::test]
async fn estimation_n_ecrit_rien(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Open).await;
    priced_sources(&db, &app).await;
    let (mut alice, home) = family(&app).await;

    let response = alice
        .post(&format!("/api/v1/households/{home}/estimate"), week_lot())
        .await;
    assert_eq!(
        response.status,
        axum::http::StatusCode::OK,
        "{}",
        response.text
    );
    let written: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM recipes WHERE slug IN ('chili', 'choux')),
                (SELECT count(*) FROM ingredients WHERE code = 'chou_de_bruxelles'),
                (SELECT count(*) FROM menus)",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(written, (0, 0, 0));
}

#[sqlx::test]
async fn estimation_lecture_seule(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Open).await;
    priced_sources(&db, &app).await;
    let (mut alice, home) = family(&app).await;
    let token = alice
        .post(
            &format!("/api/v1/households/{home}/tokens"),
            serde_json::json!({"name": "Lecture", "scopes": ["read"]}),
        )
        .await
        .json["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut claude = app.api_client(&token);
    let response = claude
        .post(&format!("/api/v1/households/{home}/estimate"), week_lot())
        .await;
    assert_eq!(
        response.status,
        axum::http::StatusCode::OK,
        "{}",
        response.text
    );
    assert_eq!(response.json["week"]["total_cents"], 2500);
}

// À revoir › Prix

const ADMIN: &str = "admin@example.org";

async fn admin_app(db: PgPool) -> TestApp {
    TestApp::with(db, |base| {
        let mut config = common::config(base, SignupPolicy::Open);
        config.admin_emails = vec![ADMIN.into()];
        config
    })
    .await
}

async fn admin(app: &TestApp) -> Browser {
    let mut admin = app.browser();
    admin.login(ADMIN).await;
    admin
}

fn issue<'a>(review: &'a Value, code: &str) -> &'a Value {
    review["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["ingredient"] == code)
        .unwrap_or_else(|| panic!("{code} absent de {review}"))
}

#[sqlx::test]
async fn revue_reservee_a_l_admin(db: PgPool) {
    let app = admin_app(db).await;
    let mut alice = alice(&app).await;
    let response = alice.get("/api/v1/community/prices").await;
    assert_eq!(
        response.status,
        axum::http::StatusCode::FORBIDDEN,
        "{}",
        response.text
    );
    let response = alice
        .post("/api/v1/community/prices/refresh", Value::Null)
        .await;
    assert_eq!(response.status, axum::http::StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn ecart_dans_la_revue(db: PgPool) {
    prices::seed_links(&db).await.unwrap();
    let app = admin_app(db.clone()).await;
    sqlx::query(
        "INSERT INTO ingredient_prices (ingredient_id, state, cents, per, source, observations, observed_on, insee_cents, flag)
         SELECT id, 'fresh', 400, 'kg', 'open_prices', 7, current_date, 221, 'divergent' FROM ingredients WHERE code = 'carrot'",
    )
    .execute(&db)
    .await
    .unwrap();
    let (mut alice, home) = home(&app).await;
    recipe(
        &mut alice,
        &home,
        "rien",
        "Rien",
        serde_json::json!([
            {"ingredient": "tofu", "qty": 100, "unit": "g", "role": "protein"},
        ]),
    )
    .await;
    sqlx::query("DELETE FROM ingredient_price_links WHERE ingredient_id = (SELECT id FROM ingredients WHERE code = 'tofu')")
        .execute(&db)
        .await
        .unwrap();

    let mut admin = admin(&app).await;
    let response = admin.get("/api/v1/community/prices").await;
    assert_eq!(
        response.status,
        axum::http::StatusCode::OK,
        "{}",
        response.text
    );
    let carrot = issue(&response.json, "carrot");
    assert_eq!(carrot["kind"], "divergent");
    assert_eq!(carrot["cents"], 400);
    assert_eq!(carrot["insee_cents"], 221);
    assert_eq!(carrot["link"]["off_category"], "en:carrots");
    assert_eq!(
        issue(&response.json, "tofu")["kind"],
        "unlinked",
        "utilisé par une recette, sans rattachement"
    );
    assert_eq!(
        issue(&response.json, "leek")["kind"],
        "missing",
        "rattaché, sans prix"
    );
}

#[sqlx::test]
async fn corriger_un_rattachement_le_rechiffre(db: PgPool) {
    prices::seed_links(&db).await.unwrap();
    let app = admin_app(db.clone()).await;
    for days in [20, 18, 16, 14, 12] {
        app.fake
            .prices
            .raw("en:baby-leeks", 5.00, days, "France", &[]);
    }
    let mut admin = admin(&app).await;
    let leek = ingredient(&mut admin, "leek").await;
    let id = leek["id"].as_str().unwrap();

    let response = admin
        .put(
            &format!("/api/v1/community/ingredients/{id}/price-links/fresh"),
            serde_json::json!({"off_category": "en:baby-leeks", "piece_g": 150}),
        )
        .await;
    assert_eq!(
        response.status,
        axum::http::StatusCode::OK,
        "{}",
        response.text
    );
    assert_eq!(
        off_category(&db, "leek").await.as_deref(),
        Some("en:baby-leeks"),
        "remplacé"
    );
    let mut prices = Value::Null;
    for _ in 0..50 {
        prices = priced(&mut admin, "leek").await;
        if prices.as_array().is_some_and(|p| !p.is_empty()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    // L'unité du poireau dans les recettes est la pièce : 5 €/kg × 150 g.
    assert_eq!(prices[0]["cents"], 75, "{prices}");
    assert_eq!(prices[0]["per"], "piece");
}

#[sqlx::test]
async fn relance_detachee(db: PgPool) {
    let app = admin_app(db.clone()).await;
    let mut admin = admin(&app).await;
    let response = admin
        .post("/api/v1/community/prices/refresh", Value::Null)
        .await;
    assert_eq!(
        response.status,
        axum::http::StatusCode::ACCEPTED,
        "{}",
        response.text
    );
    let mut runs = 0;
    for _ in 0..50 {
        runs = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM price_runs WHERE finished_at IS NOT NULL",
        )
        .fetch_one(&db)
        .await
        .unwrap();
        if runs > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(runs, 1);
    let review = admin.get("/api/v1/community/prices").await.json;
    assert_eq!(review["last_run"]["ok"], true);
}

#[sqlx::test]
async fn erreur_ponctuelle_ne_fait_pas_echouer_le_passage(db: PgPool) {
    let (state, fake) = sources(&db).await;
    carrots(&fake);
    fake.prices.fail_category("en:leeks");

    assert!(prices::run::check(&state).await);
    assert!(
        last_run_ok(&db).await,
        "un ingrédient sur cent cinquante en échec : le passage compte"
    );
    let message: String =
        sqlx::query_scalar("SELECT message FROM price_runs ORDER BY started_at DESC LIMIT 1")
            .fetch_one(&db)
            .await
            .unwrap();
    assert!(message.contains("leek"), "l'échec est noté : {message}");
    assert_eq!(stored(&db, "carrot").await.map(|p| p.0), Some(262));
    assert!(
        !prices::run::check(&state).await,
        "pas de nouveau passage avant 30 jours"
    );
}

// Relecture finale

#[sqlx::test]
async fn correction_sans_releves_retire_le_prix(db: PgPool) {
    prices::seed_links(&db).await.unwrap();
    let app = admin_app(db.clone()).await;
    sqlx::query(
        "INSERT INTO ingredient_prices (ingredient_id, state, cents, per, source, observations, observed_on, insee_cents, flag)
         SELECT id, 'fresh', 999, 'piece', 'open_prices', 5, current_date, 69, 'divergent' FROM ingredients WHERE code = 'leek'",
    )
    .execute(&db)
    .await
    .unwrap();
    let mut admin = admin(&app).await;
    let id = ingredient(&mut admin, "leek").await["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Une catégorie sans relevés, plus de série INSEE : la source répond, il n'en sort aucun prix.
    let response = admin
        .put(
            &format!("/api/v1/community/ingredients/{id}/price-links/fresh"),
            serde_json::json!({"off_category": "en:nothing-here"}),
        )
        .await;
    assert_eq!(
        response.status,
        axum::http::StatusCode::OK,
        "{}",
        response.text
    );
    let mut prices = Value::Null;
    for _ in 0..50 {
        prices = priced(&mut admin, "leek").await;
        if prices.as_array().is_some_and(|p| p.is_empty()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(
        prices,
        Value::Array(Vec::new()),
        "l'ancien prix faux est retiré"
    );
    let review = admin.get("/api/v1/community/prices").await.json;
    assert_eq!(issue(&review, "leek")["kind"], "missing");
}

#[sqlx::test]
async fn reponse_illisible_compte_comme_echec(db: PgPool) {
    let (state, fake) = sources(&db).await;
    carrots(&fake);
    prices::run::check(&state).await;
    sqlx::query("UPDATE price_runs SET started_at = now() - interval '31 days'")
        .execute(&db)
        .await
        .unwrap();

    fake.prices.set_garbled(true);
    assert!(prices::run::check(&state).await);
    assert!(
        !last_run_ok(&db).await,
        "une réponse 200 illisible n'est pas un succès"
    );
    assert_eq!(
        stored(&db, "carrot").await.map(|p| p.0),
        Some(262),
        "le prix reste"
    );
}

#[sqlx::test]
async fn relance_pendant_un_passage(db: PgPool) {
    let app = admin_app(db.clone()).await;
    sqlx::query("INSERT INTO price_runs (id) VALUES ($1)")
        .bind(uuid::Uuid::now_v7())
        .execute(&db)
        .await
        .unwrap();
    let mut admin = admin(&app).await;
    let response = admin
        .post("/api/v1/community/prices/refresh", Value::Null)
        .await;
    assert_eq!(
        response.status,
        axum::http::StatusCode::CONFLICT,
        "{}",
        response.text
    );
}

#[sqlx::test]
async fn estimation_bornee(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Open).await;
    for days in [20, 18, 16, 14, 12] {
        app.fake.prices.raw("en:roots", 4.00, days, "France", &[]);
    }
    let (mut alice, home) = family(&app).await;
    let names: Vec<String> = (1..=11).map(|i| format!("Racine {i}")).collect();
    let ingredients: Vec<Value> = names
        .iter()
        .map(|name| {
            serde_json::json!({"name": name, "aisle": "produce", "default_unit": "g", "default_state": "fresh",
                               "price_links": [{"state": "fresh", "off_category": "en:roots"}]})
        })
        .collect();
    let lines: Vec<Value> = (1..=11)
        .map(|i| serde_json::json!({"ingredient": format!("racine_{i}"), "qty": 100, "unit": "g", "role": "veg"}))
        .collect();
    let lot = serde_json::json!({
        "ingredients": ingredients,
        "recipes": [{"slug": "racines", "title": "Racines", "ingredients": lines,
                     "variants": [{"label": "Four", "active_min": 10, "total_min": 30,
                                   "phases": [{"title": "Cuire", "text": "Cuire."}]}]}],
        "menu": {"week_start": "2026-10-18", "slots": [
            {"date": "2026-10-21", "meal": "dinner", "recipe": "racines", "covers_next_lunch": false}]},
    });

    let response = alice
        .post(&format!("/api/v1/households/{home}/estimate"), lot)
        .await;
    assert_eq!(
        response.status,
        axum::http::StatusCode::OK,
        "{}",
        response.text
    );
    // Dix ingrédients nouveaux chiffrés à la volée au plus : 10 × 400 g à 4 €/kg ; le onzième reste sans prix.
    assert_eq!(
        response.json["meals"][0]["cents"], 1600,
        "{}",
        response.json
    );
    assert_eq!(response.json["meals"][0]["complete"], false);
}
