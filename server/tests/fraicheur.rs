//! Fraîcheur : jour d'achat (case cochée, ou jour habituel projeté), durée de conservation (par rayon, ou propre à
//! l'ingrédient), alerte quand le plat est cuisiné trop tard, et ses issues : congeler (rappel la veille), avancer.

mod common;

use axum::http::StatusCode;
use chaudron_server::config::SignupPolicy;
use common::{Browser, TestApp};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// Deux adultes. Saumon frais, saumon surgelé, steak haché, poulet.
async fn home(app: &TestApp) -> (Browser, Uuid) {
    let mut alice = app.browser();
    alice.login("alice@example.org").await;
    let home = alice.household("Maison").await;
    for name in ["Alice", "Bob"] {
        alice
            .post(
                &format!("/api/v1/households/{home}/members"),
                json!({"name": name, "kind": "adult"}),
            )
            .await;
    }
    let recipe = |title: &str, ingredient: &str, state: &str| {
        json!({"title": title,
               "ingredients": [{"ingredient": ingredient, "state": state, "qty": 150, "unit": "g", "role": "protein"}],
               "variants": [{"label": "Plaque", "active_min": 10, "total_min": 20,
                             "phases": [{"title": "Cuire", "text": "Cuire."}]}]})
    };
    for (slug, body) in [
        ("saumon", recipe("Saumon poêlé", "salmon", "fresh")),
        (
            "saumon-surgele",
            recipe("Saumon surgelé", "salmon", "frozen"),
        ),
        ("steak", recipe("Steak haché", "minced_beef", "fresh")),
        ("poulet", recipe("Poulet", "chicken", "fresh")),
    ] {
        let response = alice
            .put(&format!("/api/v1/households/{home}/recipes/{slug}"), body)
            .await;
        assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    }
    (alice, home)
}

fn dinner(date: &str, recipe: &str) -> Value {
    json!({"date": date, "meal": "dinner", "recipe": recipe})
}

fn freshness(menu: &Value) -> Vec<&Value> {
    menu["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|w| w["kind"] == "freshness")
        .collect()
}

#[sqlx::test]
async fn alerte_quand_le_plat_vient_trop_tard(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = home(&app).await;
    // Sans historique de courses : achat projeté la veille du premier repas, le samedi 17.
    let response = alice
        .put(
            &format!("/api/v1/households/{home}/menus/2026-10-18"),
            json!({"slots": [
                dinner("2026-10-18", "steak"),          // 1 jour : le haché tient 1 jour
                dinner("2026-10-19", "poulet"),         // 2 jours : la volaille en tient 3
                dinner("2026-10-21", "steak"),          // 4 jours : trop tard
                dinner("2026-10-22", "saumon"),         // 5 jours : le poisson en tient 2
                dinner("2026-10-23", "saumon-surgele"), // surgelé : pas d'alerte
            ]}),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    let menu = response.json;
    let warnings = freshness(&menu);
    assert_eq!(warnings.len(), 2, "{warnings:?}");

    let steak = warnings[0];
    assert_eq!(steak["date"], "2026-10-21");
    assert_eq!(steak["ingredient"], "minced_beef");
    assert_eq!(steak["purchased"], "2026-10-17");
    assert_eq!(steak["keeps_days"], 1);
    // Le plus tard possible : le lendemain de l'achat ; mais le menu commence le 18.
    assert_eq!(steak["advance_to"], "2026-10-18");

    let salmon = warnings[1];
    assert_eq!(salmon["date"], "2026-10-22");
    assert_eq!(salmon["ingredient"], "salmon");
    assert_eq!(salmon["keeps_days"], 2);
    assert_eq!(salmon["advance_to"], "2026-10-19");
    assert_eq!(salmon["slots"], json!([menu["slots"][3]["id"]]));
    let message = salmon["message"].as_str().unwrap();
    assert!(
        message.contains("samedi 17 octobre") && message.contains("2 jours"),
        "{message}"
    );
}

#[sqlx::test]
async fn jour_d_achat_d_apres_les_cases(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = home(&app).await;
    // Des dates passées : une case cochée garde l'heure du geste, bornée à l'heure du serveur.
    let url = format!("/api/v1/households/{home}/menus/2026-10-11");
    alice
        .put(
            &url,
            json!({"slots": [dinner("2026-10-11", "poulet"), dinner("2026-10-15", "saumon")]}),
        )
        .await;
    let shopping = alice.get(&format!("{url}/shopping")).await.json;
    let salmon = shopping["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["ingredient"] == "salmon")
        .unwrap()
        .clone();
    // Aux courses : le saumon est à congeler en rentrant, pour jeudi.
    assert_eq!(
        salmon["freeze"],
        json!([{"date": "2026-10-15", "recipe": "Saumon poêlé", "frozen": false}])
    );

    // Le saumon est pris le mardi 13, lors d'un deuxième passage : jeudi, il est frais.
    let response = alice
        .post(
            &format!("/api/v1/households/{home}/shopping/ops"),
            json!({"ops": [{"op_id": Uuid::now_v7(), "at": "2026-10-13T17:30:00+02:00", "week_start": "2026-10-11",
                            "kind": "check", "key": salmon["key"]}]}),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let menu = alice.get(&url).await.json;
    assert!(
        freshness(&menu).iter().all(|w| w["ingredient"] != "salmon"),
        "{menu}"
    );

    // Le menu suivant : le jour habituel des courses est désormais le mardi (la seule case cochée).
    let response = alice
        .put(
            &format!("/api/v1/households/{home}/menus/2026-10-18"),
            json!({"slots": [dinner("2026-10-18", "poulet"), dinner("2026-10-19", "saumon")]}),
        )
        .await;
    let warnings = freshness(&response.json);
    // Dernier mardi avant le premier repas (dimanche 18) : le 13 ; poulet 5 jours après, saumon 6.
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert_eq!(warnings[0]["purchased"], "2026-10-13");
}

#[sqlx::test]
async fn congeler_leve_l_alerte_et_rappelle_la_veille(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = home(&app).await;
    let url = format!("/api/v1/households/{home}/menus/2026-10-18");
    let menu = alice
        .put(
            &url,
            json!({"slots": [dinner("2026-10-18", "poulet"), dinner("2026-10-22", "saumon")]}),
        )
        .await
        .json;
    let slot = menu["slots"][1]["id"].clone();

    let response = alice
        .post(
            &format!("{url}/freeze"),
            json!({"slot": slot, "ingredient": "salmon", "frozen": true}),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let menu = response.json;
    assert!(freshness(&menu).is_empty());
    assert_eq!(
        menu["slots"][1]["thaw"],
        json!([{"ingredient": "salmon", "name": "Saumon"}])
    );
    let shopping = alice.get(&format!("{url}/shopping")).await.json;
    let salmon = shopping["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["ingredient"] == "salmon")
        .unwrap();
    assert_eq!(
        salmon["freeze"],
        json!([{"date": "2026-10-22", "recipe": "Saumon poêlé", "frozen": true}])
    );

    // Republier le menu tel quel garde la congélation (le compositeur renvoie `thaw`).
    let response = alice
        .put(&url, json!({"slots": [dinner("2026-10-18", "poulet"),
                                    {"date": "2026-10-22", "meal": "dinner", "recipe": "saumon", "thaw": ["salmon"]}]}))
        .await;
    assert_eq!(response.json["slots"][1]["thaw"][0]["ingredient"], "salmon");

    // Décongeler : l'alerte revient. Un ingrédient absent du plat : 422.
    let slot = response.json["slots"][1]["id"].clone();
    let menu = alice
        .post(
            &format!("{url}/freeze"),
            json!({"slot": slot, "ingredient": "salmon", "frozen": false}),
        )
        .await
        .json;
    assert_eq!(freshness(&menu).len(), 1);
    let response = alice
        .post(
            &format!("{url}/freeze"),
            json!({"slot": slot, "ingredient": "chicken", "frozen": true}),
        )
        .await;
    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.text
    );
}

#[sqlx::test]
async fn duree_propre_a_un_ingredient(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, _) = home(&app).await;
    // Un ingrédient importé avec sa durée : la salade tient 2 jours (le rayon en dirait 5).
    let response = alice
        .post(
            "/api/v1/ingredients",
            json!({"name": "Mâche", "aisle": "produce", "default_unit": "g", "default_state": "fresh", "keeps_days": 2}),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    assert_eq!(response.json["keeps_days"], 2);

    // Un lot précise la durée d'un ingrédient qui existe déjà : le saumon, ici, tiendrait 5 jours.
    let home = alice.me().await["households"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let response = alice
        .post(
            &format!("/api/v1/households/{home}/import"),
            json!({"ingredients": [{"name": "Saumon", "aisle": "fish", "default_unit": "g", "default_state": "fresh",
                                    "keeps_days": 5}]}),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let menu = alice
        .put(
            &format!("/api/v1/households/{home}/menus/2026-10-18"),
            json!({"slots": [dinner("2026-10-22", "saumon")]}),
        )
        .await
        .json;
    assert!(freshness(&menu).is_empty(), "{menu}");

    // Le référentiel est commun à l'instance : un lot complète une durée absente, il ne remplace pas celle qui
    // existe (le haché garde son jour).
    let response = alice
        .post(
            &format!("/api/v1/households/{home}/import"),
            json!({"ingredients": [{"name": "Bœuf haché", "aisle": "butcher", "default_unit": "g", "default_state": "fresh",
                                    "keeps_days": 9}]}),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let ingredients = alice.get("/api/v1/ingredients").await.json;
    let beef = ingredients
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["code"] == "minced_beef")
        .unwrap();
    assert_eq!(beef["keeps_days"], 1);
}
