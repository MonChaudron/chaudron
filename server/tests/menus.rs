//! Menus du foyer : créneaux, portions calculées depuis la présence ou fixées en nombre, avertissements (conflits
//! d'appareil, matériel absent, exclusions, midi sans boîtes, chevauchements), déplacer un plat, agenda, import.

mod common;

use axum::http::StatusCode;
use chaudron_server::config::SignupPolicy;
use common::{Browser, TestApp};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// Le foyer des tests : deux adultes et deux ados, qui déjeunent à la cantine sauf le mercredi et le samedi.
/// Cookeo, plaques et four ; pas de blender. Recettes : chili (Cookeo par défaut, cocotte la veille), saumon
/// (plaque), curry (Cookeo seulement), velouté (blender).
async fn home(app: &TestApp) -> (Browser, Uuid) {
    let mut alice = app.browser();
    alice.login("alice@example.org").await;
    let home = alice.household("Maison").await;
    let response = alice
        .put(
            &format!("/api/v1/households/{home}/equipment"),
            json!({"codes": ["cookeo_6l", "hob", "oven"]}),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let members = format!("/api/v1/households/{home}/members");
    for (name, kind) in [
        ("Alice", "adult"),
        ("Bob", "adult"),
        ("Léo", "teen"),
        ("Zoé", "teen"),
    ] {
        let response = alice
            .post(&members, json!({"name": name, "kind": kind}))
            .await;
        assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
        if kind == "teen" {
            let mut member = response.json;
            // Lundi … dimanche : à la maison le mercredi et le samedi midi.
            member["presence"]["lunch"] = json!([false, false, true, false, false, true, false]);
            let url = format!("{members}/{}", member["id"].as_str().unwrap());
            let response = alice.put(&url, member).await;
            assert_eq!(response.status, StatusCode::OK, "{}", response.text);
        }
    }
    for (slug, recipe) in recipes() {
        let response = alice
            .put(&format!("/api/v1/households/{home}/recipes/{slug}"), recipe)
            .await;
        assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    }
    (alice, home)
}

fn recipes() -> Vec<(&'static str, Value)> {
    let line = |ingredient: &str, qty: f32, role: &str| json!({"ingredient": ingredient, "qty": qty, "unit": "g", "role": role});
    let variant = |label: &str, equipment: Value, active: i16, day_offset: i16| {
        json!({"label": label, "equipment": equipment, "active_min": active, "total_min": 40,
               "phases": [{"title": "Cuire", "text": "Cuire.", "day_offset": day_offset}]})
    };
    vec![
        (
            "chili",
            json!({"title": "Chili", "ingredients": [line("minced_beef", 150.0, "protein"), line("rice", 50.0, "starch")],
                   "variants": [variant("Cookeo", json!(["cookeo_6l"]), 15, 0),
                                variant("Cocotte", json!(["hob"]), 20, -1)]}),
        ),
        (
            "saumon",
            json!({"title": "Saumon poêlé", "ingredients": [line("salmon", 150.0, "protein")],
                   "variants": [variant("Plaque", json!(["hob"]), 10, 0)]}),
        ),
        (
            "curry",
            json!({"title": "Curry", "ingredients": [line("chicken", 150.0, "protein")],
                   "variants": [variant("Cookeo", json!(["cookeo_6l"]), 20, 0)]}),
        ),
        (
            "veloute",
            json!({"title": "Velouté", "ingredients": [line("carrot", 300.0, "veg")],
                   "variants": [variant("Blender", json!(["blender", "hob"]), 15, 0)]}),
        ),
    ]
}

fn dinner(date: &str, recipe: &str) -> Value {
    json!({"date": date, "meal": "dinner", "recipe": recipe, "covers_next_lunch": true})
}

/// Les créneaux d'un menu, par date : « 2026-10-19 dinner saumon ».
fn plan(menu: &Value) -> Vec<String> {
    menu["slots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            format!(
                "{} {} {}",
                s["date"].as_str().unwrap(),
                s["meal"].as_str().unwrap(),
                s["recipe"]["slug"].as_str().unwrap()
            )
        })
        .collect()
}

/// Les avertissements, sauf la fraîcheur (testée à part, dans `fraicheur.rs`).
fn kinds(warnings: &Value) -> Vec<&str> {
    warnings
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["kind"].as_str().unwrap())
        .filter(|k| *k != "freshness")
        .collect()
}

#[sqlx::test]
async fn portions_calculees_depuis_la_presence(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = home(&app).await;
    let url = format!("/api/v1/households/{home}/menus/2026-10-18");

    let response = alice
        .put(
            &url,
            json!({
                "title": "Semaine du 18",
                "notes": "Un seul passage en courses.",
                "slots": [
                    // Dimanche : 4 au dîner + les 2 adultes le lundi midi.
                    dinner("2026-10-18", "chili"),
                    // Mardi : 4 au dîner + 4 le mercredi midi.
                    dinner("2026-10-20", "saumon"),
                    {"date": "2026-10-21", "meal": "dinner", "recipe": "chili", "variant": "Cocotte"},
                    // Samedi midi : tout le monde, sans boîtes.
                    {"date": "2026-10-24", "meal": "lunch", "recipe": "curry", "covers_next_lunch": false},
                ]
            }),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    let menu = alice.get(&url).await.json;
    assert_eq!(response.json, menu);
    assert_eq!(menu["week_start"], "2026-10-18");
    assert_eq!(menu["title"], "Semaine du 18");
    assert_eq!(menu["notes"], "Un seul passage en courses.");

    let slots = menu["slots"].as_array().unwrap();
    assert_eq!(slots.len(), 4);
    let sunday = &slots[0];
    assert_eq!(sunday["week_start"], "2026-10-18");
    assert_eq!(sunday["recipe"], json!({"slug": "chili", "title": "Chili"}));
    // Sans variante choisie : la variante par défaut.
    assert_eq!(sunday["variant"]["label"], "Cookeo");
    assert_eq!(sunday["diners"], 4);
    assert_eq!(sunday["next_lunch_covers"], 2);
    assert_eq!(sunday["servings"], 6);
    assert_eq!(sunday["diners_fixed"], false);
    assert_eq!(sunday["next_lunch_covers_fixed"], false);
    // Barème : les ados doublent le féculent au dîner (4 + 2 + 2 adultes en boîtes).
    assert_eq!(
        sunday["scale"],
        json!({"protein": 6.0, "starch": 8.0, "veg": 6.0, "fat": 6.0, "other": 6.0})
    );
    assert_eq!(sunday["express"], true);
    assert_eq!(sunday["prep_days_before"], 0);
    assert_eq!(sunday["portions_date"], Value::Null);

    let tuesday = &slots[1];
    assert_eq!(tuesday["servings"], 8);
    assert_eq!(tuesday["scale"]["starch"], json!(12.0));
    assert_eq!(tuesday["express"], true);

    let wednesday = &slots[2];
    assert_eq!(wednesday["variant"]["label"], "Cocotte");
    assert_eq!(wednesday["covers_next_lunch"], false);
    assert_eq!(wednesday["servings"], 4);
    assert_eq!(wednesday["express"], false);
    // Une phase la veille.
    assert_eq!(wednesday["prep_days_before"], 1);

    let saturday = &slots[3];
    assert_eq!(saturday["meal"], "lunch");
    assert_eq!(saturday["servings"], 4);
    assert_eq!(saturday["express"], false);

    // Léo déjeune à la maison le lundi : le dimanche passe à 7, sans rien republier.
    let members = alice
        .get(&format!("/api/v1/households/{home}/members"))
        .await
        .json;
    let mut leo = members[2].clone();
    leo["presence"]["lunch"][0] = json!(true);
    let response = alice
        .put(
            &format!(
                "/api/v1/households/{home}/members/{}",
                leo["id"].as_str().unwrap()
            ),
            leo,
        )
        .await;
    assert_eq!(response.status, StatusCode::OK);
    let menu = alice.get(&url).await.json;
    assert_eq!(menu["slots"][0]["servings"], 7);

    // Republier remplace le menu.
    let response = alice
        .put(
            &url,
            json!({"title": "Autre", "slots": [dinner("2026-10-19", "curry")]}),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    assert_eq!(plan(&response.json), ["2026-10-19 dinner curry"]);
    assert_eq!(response.json["notes"], "");
}

#[sqlx::test]
async fn portions_en_nombre(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = home(&app).await;
    let url = format!("/api/v1/households/{home}/menus/2026-10-18");

    let response = alice
        .put(
            &url,
            json!({"slots": [
                // Un invité jeudi soir : 5, au prorata du mélange des présents (2 adultes, 2 ados).
                {"date": "2026-10-22", "meal": "dinner", "recipe": "chili", "diners": 5},
                // Deux boîtes au lieu de quatre le mercredi midi.
                {"date": "2026-10-20", "meal": "dinner", "recipe": "chili", "covers_next_lunch": true,
                 "next_lunch_covers": 2},
            ]}),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    let slots = response.json["slots"].as_array().unwrap().clone();
    let tuesday = &slots[0];
    assert_eq!(tuesday["diners"], 4);
    assert_eq!(tuesday["next_lunch_covers"], 2);
    assert_eq!(tuesday["next_lunch_covers_fixed"], true);
    assert_eq!(tuesday["diners_fixed"], false);
    assert_eq!(tuesday["servings"], 6);
    // Dîner : féculent 6 ; boîtes : 2 sur un mélange de 4 qui fait 6, soit 3.
    assert_eq!(tuesday["scale"]["starch"], json!(9.0));
    assert_eq!(tuesday["scale"]["protein"], json!(6.0));
    let thursday = &slots[1];
    assert_eq!(thursday["diners"], 5);
    assert_eq!(thursday["diners_fixed"], true);
    assert_eq!(thursday["servings"], 5);
    assert_eq!(thursday["scale"]["protein"], json!(5.0));
    assert_eq!(thursday["scale"]["starch"], json!(7.5));

    for (slot, message) in [
        (
            json!({"date": "2026-10-22", "meal": "dinner", "recipe": "chili", "diners": -1}),
            "nombre",
        ),
        (
            json!({"date": "2026-10-17", "meal": "dinner", "recipe": "chili"}),
            "2026-10-17",
        ),
        (
            json!({"date": "2026-11-30", "meal": "dinner", "recipe": "chili"}),
            "2026-11-30",
        ),
        (
            json!({"date": "2026-10-22", "meal": "dinner", "recipe": "inconnue"}),
            "inconnue",
        ),
        (
            json!({"date": "2026-10-22", "meal": "dinner", "recipe": "chili", "variant": "Wok"}),
            "Wok",
        ),
    ] {
        let response = alice.put(&url, json!({"slots": [slot]})).await;
        assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY, "{slot}");
        assert!(
            response.json["message"].as_str().unwrap().contains(message),
            "{}",
            response.text
        );
    }
    // Rien n'a changé.
    assert_eq!(
        alice.get(&url).await.json["slots"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[sqlx::test]
async fn avertissements(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = home(&app).await;
    // Zoé est allergique au saumon.
    let ingredients = alice.get("/api/v1/ingredients").await.json;
    let salmon = ingredients
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["code"] == "salmon")
        .unwrap()["id"]
        .clone();
    let members = alice
        .get(&format!("/api/v1/households/{home}/members"))
        .await
        .json;
    let mut zoe = members[3].clone();
    zoe["exclusions"] = json!([{"ingredient_id": salmon, "kind": "allergy"}]);
    let response = alice
        .put(
            &format!(
                "/api/v1/households/{home}/members/{}",
                zoe["id"].as_str().unwrap()
            ),
            zoe,
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);

    let url = format!("/api/v1/households/{home}/menus/2026-10-18");
    let response = alice
        .put(
            &url,
            json!({"slots": [
                // Deux plats au Cookeo le même soir : le chili peut passer en cocotte.
                dinner("2026-10-18", "chili"),
                dinner("2026-10-18", "curry"),
                // Le lundi au soir et le midi, pas de conflit : ce ne sont pas les mêmes repas.
                {"date": "2026-10-19", "meal": "lunch", "recipe": "curry"},
                dinner("2026-10-19", "curry"),
                dinner("2026-10-20", "saumon"),
                dinner("2026-10-21", "veloute"),
            ]}),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    let menu = response.json;
    let slots = menu["slots"].as_array().unwrap();
    // Deux plats le même soir : rangés dans l'ordre donné.
    assert_eq!(slots[0]["position"], 0);
    assert_eq!(slots[1]["position"], 1);
    let warnings: Vec<&Value> = menu["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|w| w["kind"] != "freshness")
        .collect();
    assert_eq!(
        kinds(&menu["warnings"]),
        ["equipment_conflict", "exclusion", "missing_equipment"]
    );

    let conflict = &warnings[0];
    assert_eq!(conflict["date"], "2026-10-18");
    assert_eq!(conflict["meal"], "dinner");
    assert_eq!(conflict["equipment"], json!(["cookeo_6l"]));
    assert_eq!(conflict["slots"], json!([slots[0]["id"], slots[1]["id"]]));
    assert_eq!(
        conflict["suggestion"],
        json!({"slot": slots[0]["id"], "variant": "Cocotte"})
    );
    assert!(
        conflict["message"].as_str().unwrap().contains("Cocotte"),
        "{conflict}"
    );

    let exclusion = &warnings[1];
    assert_eq!(exclusion["date"], "2026-10-20");
    assert_eq!(exclusion["slots"], json!([slots[4]["id"]]));
    assert_eq!(exclusion["member"], "Zoé");
    assert_eq!(exclusion["ingredient"], "salmon");
    assert_eq!(exclusion["exclusion"], "allergy");

    let missing = &warnings[2];
    assert_eq!(missing["slots"], json!([slots[5]["id"]]));
    assert_eq!(missing["equipment"], json!(["blender"]));
    assert_eq!(slots[5]["variant"]["missing_equipment"], json!(["blender"]));
}

#[sqlx::test]
async fn deplacer_un_plat(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = home(&app).await;
    let url = format!("/api/v1/households/{home}/menus/2026-10-18");
    let response = alice
        .put(
            &url,
            json!({"slots": [
                dinner("2026-10-18", "chili"),
                dinner("2026-10-19", "curry"),
                dinner("2026-10-20", "saumon"),
                dinner("2026-10-21", "chili"),
                dinner("2026-10-22", "curry"),
                dinner("2026-10-23", "saumon"),
            ]}),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    let menu = response.json;
    let id = |menu: &Value, date: &str| {
        menu["slots"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["date"] == date)
            .unwrap()["id"]
            .clone()
    };
    let monday = id(&menu, "2026-10-19");
    assert_eq!(menu["slots"][1]["servings"], 6);
    let move_url = format!("{url}/move");

    // Lundi, resto : le curry du lundi va au mercredi, intercalé ; le saumon du mardi ne bouge pas.
    let response = alice
        .post(
            &move_url,
            json!({"slot": monday, "date": "2026-10-21", "meal": "dinner", "mode": "insert"}),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let menu = response.json;
    assert_eq!(
        plan(&menu),
        [
            "2026-10-18 dinner chili",
            "2026-10-20 dinner saumon",
            "2026-10-21 dinner curry",
            "2026-10-22 dinner chili",
            "2026-10-23 dinner curry",
            "2026-10-24 dinner saumon",
        ]
    );
    // Les portions restent celles des courses : le curry était pour 6 (boîtes du mardi midi), le chili du mercredi
    // pour 6, le curry du jeudi pour 6, le saumon du vendredi pour 8.
    let servings: Vec<i64> = menu["slots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["servings"].as_i64().unwrap())
        .collect();
    assert_eq!(servings, [6, 8, 6, 6, 6, 8]);
    assert_eq!(menu["slots"][2]["portions_date"], "2026-10-19");
    assert_eq!(menu["slots"][1]["portions_date"], Value::Null);
    // Le mardi midi n'a plus ses boîtes.
    let warnings: Vec<&Value> = menu["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|w| w["kind"] != "freshness")
        .collect();
    assert_eq!(kinds(&menu["warnings"]), ["lunch_without_boxes"]);
    assert_eq!(warnings[0]["date"], "2026-10-20");
    assert_eq!(warnings[0]["meal"], "lunch");
    assert_eq!(alice.get(&url).await.json, menu);

    // Échanger : le chili du dimanche et le saumon du samedi.
    let response = alice
        .post(
            &move_url,
            json!({"slot": id(&menu, "2026-10-24"), "date": "2026-10-18", "meal": "dinner", "mode": "swap"}),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let menu = response.json;
    assert_eq!(plan(&menu)[0], "2026-10-18 dinner saumon");
    assert_eq!(plan(&menu)[5], "2026-10-24 dinner chili");

    // Même soir : le chili rejoint le saumon du mardi.
    let chili = id(&menu, "2026-10-24");
    let response = alice
        .post(
            &move_url,
            json!({"slot": chili, "date": "2026-10-20", "meal": "dinner", "mode": "together"}),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let menu = response.json;
    assert_eq!(plan(&menu)[1], "2026-10-20 dinner saumon");
    assert_eq!(plan(&menu)[2], "2026-10-20 dinner chili");
    assert_eq!(menu["slots"][2]["position"], 1);

    // Vers un soir libre, quel que soit le mode.
    let response = alice
        .post(
            &move_url,
            json!({"slot": chili, "date": "2026-10-19", "meal": "dinner", "mode": "insert"}),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    assert_eq!(plan(&response.json)[1], "2026-10-19 dinner chili");
    assert_eq!(kinds(&response.json["warnings"]), Vec::<&str>::new());

    // Hors du menu, ou un créneau inconnu.
    let response = alice
        .post(
            &move_url,
            json!({"slot": chili, "date": "2026-10-17", "meal": "dinner", "mode": "insert"}),
        )
        .await;
    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.text
    );
    let response = alice
        .post(&move_url, json!({"slot": Uuid::now_v7(), "date": "2026-10-20", "meal": "dinner", "mode": "insert"}))
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND, "{}", response.text);
}

#[sqlx::test]
async fn agenda_et_historique(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = home(&app).await;
    let menus = format!("/api/v1/households/{home}/menus");
    let response = alice
        .put(
            &format!("{menus}/2026-10-11"),
            json!({"title": "Semaine du 11", "slots": [dinner("2026-10-11", "curry"), dinner("2026-10-17", "chili")]}),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    // Le menu suivant commence le samedi 17 : le dîner du 17 est prévu deux fois.
    let response = alice
        .put(
            &format!("{menus}/2026-10-17"),
            json!({"title": "Semaine du 17", "slots": [dinner("2026-10-17", "saumon"), dinner("2026-10-18", "curry")]}),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    assert_eq!(kinds(&response.json["warnings"]), ["overlap"]);
    assert_eq!(response.json["warnings"][0]["other_menu"], "2026-10-11");

    let history = alice.get(&menus).await.json;
    assert_eq!(
        history,
        json!([
            {"week_start": "2026-10-17", "title": "Semaine du 17", "first_date": "2026-10-17",
             "last_date": "2026-10-18", "slot_count": 2},
            {"week_start": "2026-10-11", "title": "Semaine du 11", "first_date": "2026-10-11",
             "last_date": "2026-10-17", "slot_count": 2},
        ])
    );

    let response = alice
        .get(&format!(
            "/api/v1/households/{home}/agenda?from=2026-10-16&days=3"
        ))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let agenda = response.json;
    assert_eq!(agenda["from"], "2026-10-16");
    assert_eq!(agenda["days"], 3);
    assert_eq!(
        plan(&agenda),
        [
            "2026-10-17 dinner chili",
            "2026-10-17 dinner saumon",
            "2026-10-18 dinner curry"
        ]
    );
    assert_eq!(agenda["slots"][0]["week_start"], "2026-10-11");
    assert_eq!(agenda["slots"][1]["week_start"], "2026-10-17");
    // Les recettes des créneaux, pour lire et cuisiner sans autre appel.
    let mut slugs: Vec<&str> = agenda["recipes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["slug"].as_str().unwrap())
        .collect();
    slugs.sort();
    assert_eq!(slugs, ["chili", "curry", "saumon"]);
    assert_eq!(kinds(&agenda["warnings"]), ["overlap", "overlap"]);

    for query in [
        "from=2026-10-16&days=0",
        "from=2026-10-16&days=60",
        "from=demain&days=3",
    ] {
        let response = alice
            .get(&format!("/api/v1/households/{home}/agenda?{query}"))
            .await;
        assert_eq!(response.status.as_u16() / 100, 4, "{query}");
    }

    // Supprimer un menu.
    let response = alice.delete(&format!("{menus}/2026-10-11")).await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert_eq!(
        alice.get(&format!("{menus}/2026-10-11")).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        alice.delete(&format!("{menus}/2026-10-11")).await.status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
async fn une_recette_d_un_menu_ne_se_supprime_pas(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = home(&app).await;
    let menu = format!("/api/v1/households/{home}/menus/2026-10-18");
    alice
        .put(&menu, json!({"slots": [dinner("2026-10-18", "chili")]}))
        .await;
    let recipe = format!("/api/v1/households/{home}/recipes/chili");

    let response = alice.delete(&recipe).await;
    assert_eq!(response.status, StatusCode::CONFLICT, "{}", response.text);
    assert_eq!(response.json["error"], "recipe_in_menu");
    // Republier la recette garde le menu (la variante est retrouvée par son nom).
    let (_, chili) = recipes().into_iter().next().unwrap();
    assert_eq!(alice.put(&recipe, chili).await.status, StatusCode::OK);
    assert_eq!(
        alice.get(&menu).await.json["slots"][0]["variant"]["label"],
        "Cookeo"
    );

    alice.delete(&menu).await;
    assert_eq!(alice.delete(&recipe).await.status, StatusCode::NO_CONTENT);

    // Un foyer qui a des menus se supprime.
    alice
        .put(&menu, json!({"slots": [dinner("2026-10-18", "curry")]}))
        .await;
    let response = alice.delete(&format!("/api/v1/households/{home}")).await;
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", response.text);
}

#[sqlx::test]
async fn menus_d_un_autre_foyer_invisibles(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Open).await;
    let (mut alice, home) = home(&app).await;
    let menu = format!("/api/v1/households/{home}/menus/2026-10-18");
    alice
        .put(&menu, json!({"slots": [dinner("2026-10-18", "chili")]}))
        .await;

    let mut bob = app.browser();
    bob.login("bob@example.org").await;
    let bob_home = bob.household("Chez Bob").await;
    // Le même `week_start` existe indépendamment dans chaque foyer.
    let response = bob
        .put(
            &format!("/api/v1/households/{bob_home}/menus/2026-10-18"),
            json!({"slots": []}),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    assert_eq!(bob.get(&menu).await.status, StatusCode::NOT_FOUND);
    assert_eq!(
        bob.put(&menu, json!({"slots": []})).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        bob.get(&format!(
            "/api/v1/households/{home}/agenda?from=2026-10-18&days=7"
        ))
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(bob.delete(&menu).await.status, StatusCode::NOT_FOUND);
}

async fn token(browser: &mut Browser, home: Uuid, scopes: Value) -> String {
    let response = browser
        .post(
            &format!("/api/v1/households/{home}/tokens"),
            json!({"name": "Claude", "scopes": scopes}),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    response.json["token"].as_str().unwrap().to_owned()
}

fn lot(slots: Value) -> Value {
    let (_, mut tajine) = recipes().into_iter().nth(2).unwrap();
    tajine["slug"] = json!("tajine");
    tajine["title"] = json!("Tajine");
    json!({
        "recipes": [tajine],
        "menu": {"week_start": "2026-10-18", "title": "Lot", "notes": "Les boîtes.", "slots": slots},
    })
}

#[sqlx::test]
async fn importer_un_menu(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = home(&app).await;
    let import = format!("/api/v1/households/{home}/import");
    let menu = format!("/api/v1/households/{home}/menus/2026-10-18");
    // Le format des lots : `servings` total, `variant` par son nom.
    let slots = json!([
        {"date": "2026-10-18", "meal": "dinner", "recipe": "tajine", "variant": "Cookeo",
         "covers_next_lunch": true, "servings": 6},
        {"date": "2026-10-20", "meal": "dinner", "recipe": "chili", "variant": "Cocotte",
         "covers_next_lunch": true, "servings": 8},
        // Jeudi : 4 + 2 attendus ; le lot en dit 7, soit 3 boîtes.
        {"date": "2026-10-22", "meal": "dinner", "recipe": "chili", "variant": "Cookeo",
         "covers_next_lunch": true, "servings": 7},
        // Sans boîtes, 3 au lieu de 4.
        {"date": "2026-10-23", "meal": "dinner", "recipe": "saumon", "variant": "Plaque",
         "covers_next_lunch": false, "servings": 3},
    ]);

    // Sans le droit de publier des menus : rien n'est écrit, pas même la recette.
    let secret = token(&mut alice, home, json!(["recipes:write"])).await;
    let response = app
        .api_client(&secret)
        .post(&import, lot(slots.clone()))
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN, "{}", response.text);
    assert_eq!(
        alice
            .get(&format!("/api/v1/households/{home}/recipes/tajine"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    let secret = token(&mut alice, home, json!(["recipes:write", "menus:write"])).await;
    let mut claude = app.api_client(&secret);
    let response = claude.post(&import, lot(slots.clone())).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    assert_eq!(
        response.json,
        json!({
            "ingredients_created": [],
            "recipes_created": ["tajine"],
            "recipes_updated": [],
            "recipe_warnings": [],
            "menu_imported": true,
            "warnings": response.json["warnings"],
        })
    );
    assert_eq!(kinds(&response.json["warnings"]), Vec::<&str>::new());
    let read = alice.get(&menu).await.json;
    assert_eq!(read["title"], "Lot");
    assert_eq!(read["notes"], "Les boîtes.");
    let slots_read = read["slots"].as_array().unwrap();
    let servings: Vec<i64> = slots_read
        .iter()
        .map(|s| s["servings"].as_i64().unwrap())
        .collect();
    assert_eq!(servings, [6, 8, 7, 3]);
    // Ce qui suit la présence n'est pas figé ; seul l'écart l'est.
    assert_eq!(slots_read[0]["diners_fixed"], false);
    assert_eq!(slots_read[0]["next_lunch_covers_fixed"], false);
    assert_eq!(slots_read[2]["diners_fixed"], false);
    assert_eq!(slots_read[2]["next_lunch_covers"], 3);
    assert_eq!(slots_read[2]["next_lunch_covers_fixed"], true);
    assert_eq!(slots_read[3]["diners"], 3);
    assert_eq!(slots_read[3]["diners_fixed"], true);

    // Republier : idempotent.
    let response = claude.post(&import, lot(slots.clone())).await;
    assert_eq!(response.json["recipes_updated"], json!(["tajine"]));
    assert_eq!(response.json["menu_imported"], true);
    assert_eq!(
        alice.get(&menu).await.json["slots"]
            .as_array()
            .unwrap()
            .len(),
        4
    );

    // Les avertissements sont rendus à Claude.
    let conflict = json!([
        dinner("2026-10-18", "tajine"),
        dinner("2026-10-18", "chili")
    ]);
    let response = claude.post(&import, lot(conflict)).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    assert_eq!(kinds(&response.json["warnings"]), ["equipment_conflict"]);

    // Une variante inconnue : 422, rien n'est écrit.
    let wrong =
        json!([{"date": "2026-10-18", "meal": "dinner", "recipe": "chili", "variant": "Wok"}]);
    let response = claude.post(&import, lot(wrong)).await;
    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.text
    );
    assert!(response.json["message"].as_str().unwrap().contains("Wok"));
    assert_eq!(
        alice.get(&menu).await.json["slots"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // Un menu seul ne demande que le droit des menus.
    let secret = token(&mut alice, home, json!(["menus:write"])).await;
    let response = app
        .api_client(&secret)
        .post(
            &import,
            json!({"menu": {"week_start": "2026-10-25", "slots": [dinner("2026-10-25", "chili")]}}),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    assert_eq!(response.json["menu_imported"], true);
}
