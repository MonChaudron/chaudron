//! Référentiel d'ingrédients adossé à Ciqual : chargement de la table, recherche, ingrédients et leurs états.

mod common;

use axum::http::StatusCode;
use chaudron_server::{ciqual, config::SignupPolicy};
use common::{Browser, TestApp};
use serde_json::{Value, json};
use sqlx::PgPool;

async fn alice(app: &TestApp) -> Browser {
    let mut alice = app.browser();
    alice.login("alice@example.org").await;
    alice
}

fn names(list: &Value) -> Vec<&str> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect()
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

#[sqlx::test]
async fn ciqual_charge_une_fois_par_version(db: PgPool) {
    assert!(ciqual::sync(&db).await.unwrap(), "premier chargement");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM foods")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 3484);
    assert!(
        !ciqual::sync(&db).await.unwrap(),
        "même version : rien à faire"
    );

    let (name, kcal, protein): (String, Option<f32>, Option<f32>) =
        sqlx::query_as("SELECT name, energy_kcal, protein_g FROM foods WHERE code = 20009")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(name, "Carotte, crue");
    assert_eq!(kcal, Some(30.2));
    assert_eq!(protein, Some(0.78));
}

#[sqlx::test]
async fn ciqual_garde_les_lignes_reliees_qui_disparaissent(db: PgPool) {
    ciqual::sync(&db).await.unwrap();
    // Une ligne d'une ancienne version, encore reliée à un ingrédient, et une autre qui ne l'est pas.
    sqlx::query(
        "INSERT INTO foods (code, name, search_key, group_code, subgroup_code) VALUES
            (999001, 'Ancienne, reliée', 'ancienne, reliee', '02', '0201'),
            (999002, 'Ancienne, seule', 'ancienne, seule', '02', '0201')",
    )
    .execute(&db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ingredient_foods (ingredient_id, state, food_code)
         SELECT id, 'cooked', 999001 FROM ingredients WHERE code = 'carrot'",
    )
    .execute(&db)
    .await
    .unwrap();
    sqlx::query("UPDATE reference_data SET version = 'ancienne' WHERE name = 'ciqual'")
        .execute(&db)
        .await
        .unwrap();

    assert!(ciqual::sync(&db).await.unwrap());
    let codes: Vec<i32> =
        sqlx::query_scalar("SELECT code FROM foods WHERE code > 999000 ORDER BY code")
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(codes, [999001]);
}

#[sqlx::test]
async fn recherche_dans_ciqual_sans_accents(db: PgPool) {
    ciqual::sync(&db).await.unwrap();
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let mut alice = alice(&app).await;

    // Tous les mots, dans n'importe quel ordre, sans accents ; les noms qui commencent par le premier mot d'abord.
    let response = alice.get("/api/v1/foods?q=carot%20cru").await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let found = names(&response.json);
    assert_eq!(found[0], "Carotte, crue");
    assert!(found.iter().all(|n| n.to_lowercase().contains("cru")));

    let found = alice.get("/api/v1/foods?q=EPINARD%20surgele").await.json;
    assert_eq!(
        names(&found),
        ["Epinard, haché ou purée ou en branches, surgelé, cru"]
    );
    assert_eq!(found[0]["code"], 20083);
    assert_eq!(found[0]["energy_kcal"], json!(28.0));

    let found = alice.get("/api/v1/foods?q=boeuf&limit=3").await.json;
    assert_eq!(found.as_array().unwrap().len(), 3);

    let response = alice.get("/api/v1/foods?q=%20").await;
    assert_eq!(response.json, json!([]));

    let mut anonymous = app.browser();
    let response = anonymous.get("/api/v1/foods?q=carotte").await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn ingredients_avec_nom_etat_par_defaut_et_ciqual(db: PgPool) {
    ciqual::sync(&db).await.unwrap();
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let mut alice = alice(&app).await;

    let carrot = ingredient(&mut alice, "carrot").await;
    assert_eq!(carrot["name"], "Carotte");
    assert_eq!(carrot["aisle"], "produce");
    assert_eq!(carrot["default_state"], "fresh");
    assert_eq!(
        carrot["foods"],
        json!([
            {"state": "fresh", "food_code": 20009, "food_name": "Carotte, crue"},
            {"state": "frozen", "food_code": 20208, "food_name": "Carotte, surgelée, crue"},
            {"state": "canned", "food_code": 20007, "food_name": "Carotte, appertisée, égouttée"},
            {"state": "dried", "food_code": 20092, "food_name": "Carotte, déshydratée"},
        ])
    );
    assert_eq!(
        ingredient(&mut alice, "lentil").await["default_state"],
        "dried"
    );
    assert_eq!(
        ingredient(&mut alice, "chickpea").await["default_state"],
        "canned"
    );
    assert_eq!(
        ingredient(&mut alice, "shrimp").await["default_state"],
        "cooked"
    );
    assert_eq!(ingredient(&mut alice, "egg").await["name"], "Œuf");

    // Chaque ingrédient du M1 a au moins sa ligne Ciqual pour son état par défaut.
    let list = alice.get("/api/v1/ingredients").await.json;
    for i in list.as_array().unwrap() {
        let foods = i["foods"].as_array().unwrap();
        assert!(
            foods.iter().any(|f| f["state"] == i["default_state"]),
            "{}",
            i["code"]
        );
    }
}

#[sqlx::test]
async fn creer_un_ingredient_depuis_ciqual(db: PgPool) {
    ciqual::sync(&db).await.unwrap();
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let mut alice = alice(&app).await;

    let response = alice
        .post(
            "/api/v1/ingredients",
            json!({
                "name": " Petits pois ",
                "aisle": "produce",
                "default_unit": "g",
                "default_state": "frozen",
                "foods": [
                    {"state": "frozen", "food_code": 20084},
                    {"state": "canned", "food_code": 20036},
                ],
            }),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    let peas = response.json;
    assert_eq!(peas["code"], "petits_pois");
    assert_eq!(peas["name"], "Petits pois");
    assert_eq!(peas["default_state"], "frozen");
    assert_eq!(peas["foods"][0]["state"], "frozen");
    assert_eq!(peas["foods"][0]["food_name"], "Petits pois, surgelés, crus");
    assert_eq!(ingredient(&mut alice, "petits_pois").await, peas);

    // Même nom (aux accents et à la casse près) : conflit, l'ingrédient existe déjà.
    let response = alice
        .post(
            "/api/v1/ingredients",
            json!({"name": "petits POIS", "aisle": "frozen", "default_unit": "g", "default_state": "frozen"}),
        )
        .await;
    assert_eq!(response.status, StatusCode::CONFLICT);
    assert_eq!(response.json["error"], "conflict");

    // Le code est tiré du nom, sans accents.
    let response = alice
        .post(
            "/api/v1/ingredients",
            json!({"name": "Crème fraîche épaisse", "aisle": "dairy", "default_unit": "g", "default_state": "fresh"}),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    assert_eq!(response.json["code"], "creme_fraiche_epaisse");
    assert_eq!(response.json["foods"], json!([]));

    for body in [
        json!({"name": "", "aisle": "produce", "default_unit": "g", "default_state": "fresh"}),
        json!({"name": "Navet", "aisle": "produce", "default_unit": "g", "default_state": "fresh",
               "foods": [{"state": "fresh", "food_code": 1}]}),
        json!({"name": "Navet", "aisle": "produce", "default_unit": "g", "default_state": "fresh",
               "foods": [{"state": "fresh", "food_code": 20009}, {"state": "fresh", "food_code": 20034}]}),
    ] {
        let response = alice.post("/api/v1/ingredients", body).await;
        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{}",
            response.text
        );
    }
}

#[sqlx::test]
async fn relier_un_etat_a_ciqual(db: PgPool) {
    ciqual::sync(&db).await.unwrap();
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let mut alice = alice(&app).await;
    let id = ingredient(&mut alice, "salmon").await["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let response = alice
        .put(
            &format!("/api/v1/ingredients/{id}/foods/cooked"),
            json!({"food_code": 25996}),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    assert_eq!(
        response.json["foods"],
        json!([
            {"state": "fresh", "food_code": 26036, "food_name": "Saumon, élevage, cru"},
            {"state": "cooked", "food_code": 25996, "food_name": "Saumon, cuit, sans précision (aliment moyen)"},
        ])
    );

    // Remplacer la ligne d'un état.
    let response = alice
        .put(
            &format!("/api/v1/ingredients/{id}/foods/cooked"),
            json!({"food_code": 26037}),
        )
        .await;
    assert_eq!(response.json["foods"][1]["food_code"], 26037);

    let response = alice
        .put(
            &format!("/api/v1/ingredients/{id}/foods/cooked"),
            json!({"food_code": 1}),
        )
        .await;
    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    let response = alice
        .put(
            &format!("/api/v1/ingredients/{}/foods/cooked", uuid::Uuid::now_v7()),
            json!({"food_code": 26037}),
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
}

fn bulgur_lot(cooked_ratio: f64) -> Value {
    json!({"ingredients": [
        {"name": "Boulgour", "aisle": "grocery", "default_unit": "g", "default_state": "dried",
         "cooked_ratio": cooked_ratio},
    ]})
}

#[sqlx::test]
async fn lot_complete_cooked_ratio(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Open).await;
    let mut alice = alice(&app).await;
    let home = alice.household("Maison").await.to_string();
    let import = format!("/api/v1/households/{home}/import");

    let response = alice.post(&import, bulgur_lot(2.4)).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let bulgur = ingredient(&mut alice, "boulgour").await;
    assert!((bulgur["cooked_ratio"].as_f64().unwrap() - 2.4).abs() < 1e-6);

    alice.post(&import, bulgur_lot(3.0)).await;
    let bulgur = ingredient(&mut alice, "boulgour").await;
    assert!(
        (bulgur["cooked_ratio"].as_f64().unwrap() - 2.4).abs() < 1e-6,
        "un rendement connu n'est jamais remplacé"
    );

    let response = alice.post(&import, bulgur_lot(0.0)).await;
    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
}
