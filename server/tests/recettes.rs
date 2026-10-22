//! Recettes du foyer : publication (PUT idempotent), lecture, recherche, suppression, jetons d'API, import en lot.

mod common;

use axum::http::StatusCode;
use chaudron_server::{ciqual, config::SignupPolicy};
use common::{Browser, TestApp};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// Alice, propriétaire du foyer « Maison », qui a un four et des plaques (pas de Cookeo).
async fn alice_and_home(app: &TestApp) -> (Browser, Uuid) {
    let mut alice = app.browser();
    alice.login("alice@example.org").await;
    let home = alice.household("Maison").await;
    let response = alice
        .put(
            &format!("/api/v1/households/{home}/equipment"),
            json!({"codes": ["oven", "hob"]}),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK);
    (alice, home)
}

/// Chili con carne, pour une portion standard : variante Cookeo par défaut, variante cocotte.
fn chili() -> Value {
    json!({
        "title": "Chili con carne",
        "description": "Le chili du dimanche, pour le dîner et les boîtes du lendemain.",
        "tags": ["batch", "épicé"],
        "season_months": [10, 11, 12, 1, 2],
        "source": "Menu du 6 septembre 2026",
        "ingredients": [
            {"ingredient": "minced_beef", "qty": 150, "unit": "g", "role": "protein"},
            {"ingredient": "onion", "qty": 0.5, "unit": "piece", "role": "veg"},
            {"ingredient": "tomato", "state": "canned", "qty": 0.08, "unit": "kg", "role": "veg",
             "note": "pelées, en boîte"},
            {"ingredient": "rice", "qty": 50, "unit": "g", "role": "starch"},
            {"ingredient": "garlic", "qty": 1, "unit": "piece", "role": "other", "per_batch": true},
            {"ingredient": "coriander", "unit": "g", "role": "other", "optional": true, "note": "au goût"},
            {"ingredient": "milk", "qty": 2, "unit": "cl", "role": "other"},
        ],
        "variants": [
            {
                "label": "Cookeo",
                "equipment": ["cookeo_6l"],
                "active_min": 15,
                "total_min": 35,
                "is_default": true,
                "phases": [
                    {
                        "title": "Dorer",
                        "text": "Dorer l'oignon et le bœuf, 5 min.",
                        "steps": [
                            {"text": "Émincer l'oignon.", "spoken_text": "Émince l'oignon."},
                            {"text": "Dorer 5 min.", "timer_s": 300, "equipment": "cookeo_6l",
                             "params": {"program": "brown"}},
                        ]
                    },
                    {"title": "Sous pression", "text": "Tomates, sous pression 12 min.", "timer_s": 720},
                ]
            },
            {
                "label": "Cocotte",
                "equipment": ["hob"],
                "active_min": 20,
                "total_min": 60,
                "vessels": 2,
                "phases": [
                    {"title": "Mijoter", "text": "Tout en cocotte, 45 min à feu doux.", "day_offset": -1},
                ]
            },
        ]
    })
}

#[sqlx::test]
async fn publier_et_lire_une_recette(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = alice_and_home(&app).await;
    let url = format!("/api/v1/households/{home}/recipes/chili-con-carne");

    let response = alice.put(&url, chili()).await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    let recipe = alice.get(&url).await.json;
    assert_eq!(response.json, recipe);

    assert_eq!(recipe["slug"], "chili-con-carne");
    assert_eq!(recipe["title"], "Chili con carne");
    assert_eq!(recipe["tags"], json!(["batch", "épicé"]));
    assert_eq!(recipe["season_months"], json!([10, 11, 12, 1, 2]));
    assert_eq!(recipe["source"], "Menu du 6 septembre 2026");

    let lines = recipe["ingredients"].as_array().unwrap();
    assert_eq!(lines.len(), 7);
    // L'état par défaut vient de l'ingrédient (le riz est sec) ; le nom vient du référentiel.
    assert_eq!(lines[0]["ingredient"], "minced_beef");
    assert_eq!(lines[0]["name"], "Bœuf haché");
    assert_eq!(lines[0]["state"], "fresh");
    assert_eq!(lines[0]["qty"], json!(150.0));
    assert_eq!(lines[0]["role"], "protein");
    assert_eq!(lines[0]["optional"], false);
    assert_eq!(lines[0]["per_batch"], false);
    assert_eq!(lines[3]["state"], "dried");
    // kg et cl convertis en g et ml.
    assert_eq!(lines[2]["state"], "canned");
    assert_eq!(lines[2]["qty"], json!(80.0));
    assert_eq!(lines[2]["unit"], "g");
    assert_eq!(lines[2]["note"], "pelées, en boîte");
    assert_eq!(lines[6]["qty"], json!(20.0));
    assert_eq!(lines[6]["unit"], "ml");
    assert_eq!(lines[4]["per_batch"], true);
    // Sans quantité : « au goût ».
    assert_eq!(lines[5]["qty"], Value::Null);
    assert_eq!(lines[5]["optional"], true);

    let variants = recipe["variants"].as_array().unwrap();
    assert_eq!(variants.len(), 2);
    let cookeo = &variants[0];
    assert_eq!(cookeo["label"], "Cookeo");
    assert_eq!(cookeo["is_default"], true);
    assert_eq!(cookeo["vessels"], 1);
    assert_eq!(cookeo["active_min"], 15);
    assert_eq!(cookeo["total_min"], 35);
    // Le foyer n'a pas de Cookeo.
    assert_eq!(cookeo["missing_equipment"], json!(["cookeo_6l"]));
    assert_eq!(cookeo["phases"][0]["title"], "Dorer");
    assert_eq!(cookeo["phases"][0]["day_offset"], 0);
    assert_eq!(cookeo["phases"][0]["steps"][1]["timer_s"], 300);
    assert_eq!(cookeo["phases"][0]["steps"][1]["equipment"], "cookeo_6l");
    assert_eq!(
        cookeo["phases"][0]["steps"][1]["params"],
        json!({"program": "brown"})
    );
    assert_eq!(
        cookeo["phases"][0]["steps"][0]["spoken_text"],
        "Émince l'oignon."
    );
    assert_eq!(cookeo["phases"][1]["timer_s"], 720);
    assert_eq!(cookeo["phases"][1]["steps"], json!([]));
    let cocotte = &variants[1];
    assert_eq!(cocotte["is_default"], false);
    assert_eq!(cocotte["vessels"], 2);
    assert_eq!(cocotte["missing_equipment"], json!([]));
    assert_eq!(cocotte["phases"][0]["day_offset"], -1);
}

#[sqlx::test]
async fn republier_remplace_la_recette(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = alice_and_home(&app).await;
    let url = format!("/api/v1/households/{home}/recipes/chili-con-carne");

    alice.put(&url, chili()).await;
    let first = alice.get(&url).await.json;
    // Même contenu : 200, rien ne change.
    let response = alice.put(&url, chili()).await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.json["ingredients"], first["ingredients"]);

    let mut changed = chili();
    changed["title"] = json!("Chili");
    changed["ingredients"] =
        json!([{"ingredient": "lentil", "qty": 60, "unit": "g", "role": "protein"}]);
    changed["variants"] = json!([{"label": "Cocotte", "equipment": ["hob"], "active_min": 10, "total_min": 50,
                                  "phases": [{"title": "Tout", "text": "Mijoter."}]}]);
    let response = alice.put(&url, changed).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let recipe = alice.get(&url).await.json;
    assert_eq!(recipe["title"], "Chili");
    assert_eq!(recipe["ingredients"].as_array().unwrap().len(), 1);
    // Une seule variante : c'est celle par défaut.
    assert_eq!(recipe["variants"][0]["is_default"], true);

    let list = alice
        .get(&format!("/api/v1/households/{home}/recipes"))
        .await
        .json;
    assert_eq!(list.as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn recettes_invalides(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = alice_and_home(&app).await;
    let url = format!("/api/v1/households/{home}/recipes/chili");

    let mut cases: Vec<(&str, Value)> = Vec::new();
    let mut unknown = chili();
    unknown["ingredients"][0]["ingredient"] = json!("licorne");
    cases.push(("licorne", unknown));
    let mut empty_title = chili();
    empty_title["title"] = json!("  ");
    cases.push(("titre", empty_title));
    let mut no_variant = chili();
    no_variant["variants"] = json!([]);
    cases.push(("variante", no_variant));
    let mut two_defaults = chili();
    two_defaults["variants"][1]["is_default"] = json!(true);
    cases.push(("par défaut", two_defaults));
    let mut bad_equipment = chili();
    bad_equipment["variants"][0]["equipment"] = json!(["fusee"]);
    cases.push(("fusee", bad_equipment));
    let mut bad_month = chili();
    bad_month["season_months"] = json!([13]);
    cases.push(("mois", bad_month));
    let mut zero_qty = chili();
    zero_qty["ingredients"][0]["qty"] = json!(0);
    cases.push(("quantité", zero_qty));
    let mut times = chili();
    times["variants"][0]["active_min"] = json!(50);
    cases.push(("temps", times));
    let mut no_phase = chili();
    no_phase["variants"][0]["phases"] = json!([]);
    cases.push(("phase", no_phase));
    let mut bad_unit = chili();
    bad_unit["ingredients"][0]["unit"] = json!("poignée");
    cases.push(("unit", bad_unit));

    for (expected, body) in cases {
        let response = alice.put(&url, body).await;
        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{expected} : {}",
            response.text
        );
        assert!(
            response
                .text
                .to_lowercase()
                .contains(&expected.to_lowercase()),
            "{expected} : {}",
            response.text
        );
    }

    for slug in ["Chili", "chili_con", "-chili", "chili--x", &"a".repeat(81)] {
        let response = alice
            .put(
                &format!("/api/v1/households/{home}/recipes/{slug}"),
                chili(),
            )
            .await;
        assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY, "{slug}");
    }
    // Rien n'a été écrit.
    let list = alice
        .get(&format!("/api/v1/households/{home}/recipes"))
        .await
        .json;
    assert_eq!(list, json!([]));
}

#[sqlx::test]
async fn rechercher_les_recettes(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = alice_and_home(&app).await;
    let recipes = format!("/api/v1/households/{home}/recipes");
    alice
        .put(&format!("{recipes}/chili-con-carne"), chili())
        .await;
    let mut dahl = chili();
    dahl["title"] = json!("Dahl de lentilles corail");
    dahl["tags"] = json!(["végétarien"]);
    dahl["ingredients"] =
        json!([{"ingredient": "lentil", "qty": 60, "unit": "g", "role": "protein"}]);
    alice.put(&format!("{recipes}/dahl"), dahl).await;

    let list = alice.get(&recipes).await.json;
    let titles: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Chili con carne", "Dahl de lentilles corail"]);
    // Le résumé donne la variante par défaut.
    assert_eq!(list[0]["slug"], "chili-con-carne");
    assert_eq!(list[0]["default_variant"]["label"], "Cookeo");
    assert_eq!(list[0]["default_variant"]["active_min"], 15);
    assert_eq!(list[0]["default_variant"]["total_min"], 35);
    assert_eq!(
        list[0]["default_variant"]["equipment"],
        json!(["cookeo_6l"])
    );
    assert_eq!(list[0]["variant_count"], 2);
    assert_eq!(list[0]["tags"], json!(["batch", "épicé"]));

    let search = |q: &str| format!("{recipes}?q={q}");
    let found = alice.get(&search("CHILI")).await.json;
    assert_eq!(found.as_array().unwrap().len(), 1);
    assert_eq!(found[0]["slug"], "chili-con-carne");
    // Par ingrédient, sans accents.
    let found = alice.get(&search("lentilles")).await.json;
    assert_eq!(found[0]["slug"], "dahl");
    let found = alice.get(&search("boeuf%20hache")).await.json;
    assert_eq!(found[0]["slug"], "chili-con-carne");
    let found = alice
        .get(&format!("{recipes}?tag=v%C3%A9g%C3%A9tarien"))
        .await
        .json;
    assert_eq!(found.as_array().unwrap().len(), 1);
    assert_eq!(found[0]["slug"], "dahl");
}

#[sqlx::test]
async fn supprimer_une_recette(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = alice_and_home(&app).await;
    let url = format!("/api/v1/households/{home}/recipes/chili-con-carne");
    alice.put(&url, chili()).await;

    let response = alice.delete(&url).await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert_eq!(alice.get(&url).await.status, StatusCode::NOT_FOUND);
    assert_eq!(alice.delete(&url).await.status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn recettes_d_un_autre_foyer_invisibles(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Open).await;
    let (mut alice, home) = alice_and_home(&app).await;
    let url = format!("/api/v1/households/{home}/recipes/chili-con-carne");
    alice.put(&url, chili()).await;

    let mut eve = app.browser();
    eve.login("eve@example.org").await;
    let eve_home = eve.household("Chez Eve").await;
    assert_eq!(eve.get(&url).await.status, StatusCode::NOT_FOUND);
    assert_eq!(eve.put(&url, chili()).await.status, StatusCode::NOT_FOUND);
    assert_eq!(eve.delete(&url).await.status, StatusCode::NOT_FOUND);
    assert_eq!(
        eve.get(&format!("/api/v1/households/{home}/recipes"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    // Le même slug existe indépendamment dans chaque foyer.
    let response = eve
        .put(
            &format!("/api/v1/households/{eve_home}/recipes/chili-con-carne"),
            chili(),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED);
}

/// Crée un jeton et renvoie sa valeur (montrée une seule fois).
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

#[sqlx::test]
async fn jetons_d_api(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Closed).await;
    let (mut alice, home) = alice_and_home(&app).await;
    let tokens = format!("/api/v1/households/{home}/tokens");
    let recipe = format!("/api/v1/households/{home}/recipes/chili-con-carne");

    let secret = token(&mut alice, home, json!(["read", "recipes:write"])).await;
    assert!(secret.starts_with("chd_"), "{secret}");
    // Seul le hash est stocké.
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM api_tokens WHERE token_hash = $1")
        .bind(secret.as_bytes())
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(stored, 0);

    let mut claude = app.api_client(&secret);
    let response = claude.put(&recipe, chili()).await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    assert_eq!(claude.get(&recipe).await.status, StatusCode::OK);
    // Le référentiel aussi, pour construire un lot.
    assert_eq!(
        claude.get("/api/v1/ingredients").await.status,
        StatusCode::OK
    );
    // Un jeton n'est pas un compte.
    assert_eq!(
        claude.get("/api/v1/me").await.status,
        StatusCode::UNAUTHORIZED
    );

    let list = alice.get(&tokens).await.json;
    assert_eq!(list[0]["name"], "Claude");
    assert_eq!(list[0]["scopes"], json!(["read", "recipes:write"]));
    assert!(list[0]["last_used_at"].is_string());
    assert_eq!(list[0].get("token"), None);

    // Lecture seule.
    let reader = token(&mut alice, home, json!(["read"])).await;
    let mut reader = app.api_client(&reader);
    assert_eq!(reader.get(&recipe).await.status, StatusCode::OK);
    assert_eq!(
        reader.put(&recipe, chili()).await.status,
        StatusCode::FORBIDDEN
    );

    // Un autre foyer : invisible.
    let other = alice.household("Chalet").await;
    assert_eq!(
        claude
            .get(&format!("/api/v1/households/{other}/recipes"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // Un membre qui n'est pas propriétaire ne gère pas les jetons.
    let invite = alice.invite(home).await;
    let mut bob = app.browser();
    bob.login_with("bob@example.org", Some(&invite)).await;
    assert_eq!(
        bob.post(&tokens, json!({"name": "Bob", "scopes": ["read"]}))
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    // Révocation.
    let id = list[0]["id"].as_str().unwrap();
    let response = alice.delete(&format!("{tokens}/{id}")).await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert_eq!(claude.get(&recipe).await.status, StatusCode::UNAUTHORIZED);

    for body in [
        json!({"name": "", "scopes": ["read"]}),
        json!({"name": "X", "scopes": []}),
        json!({"name": "X", "scopes": ["admin"]}),
    ] {
        let response = alice.post(&tokens, body).await;
        assert!(response.status.is_client_error(), "{}", response.text);
    }
}

fn lot() -> Value {
    let mut dahl = chili();
    dahl["slug"] = json!("dahl");
    dahl["title"] = json!("Dahl");
    dahl["ingredients"] = json!([
        {"ingredient": "patate_douce", "qty": 120, "unit": "g", "role": "veg"},
        {"ingredient": "lentil", "qty": 60, "unit": "g", "role": "protein"},
    ]);
    let mut chili = chili();
    chili["slug"] = json!("chili-con-carne");
    json!({
        "ingredients": [
            {"name": "Patate douce", "aisle": "produce", "default_unit": "g", "default_state": "fresh",
             "foods": [{"state": "fresh", "food_code": 4101}]},
            {"name": "Lentilles", "aisle": "grocery", "default_unit": "g", "default_state": "dried"},
        ],
        "recipes": [chili, dahl],
    })
}

#[sqlx::test]
async fn importer_un_lot(db: PgPool) {
    ciqual::sync(&db).await.unwrap();
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = alice_and_home(&app).await;
    let import = format!("/api/v1/households/{home}/import");
    let secret = token(&mut alice, home, json!(["recipes:write"])).await;
    let mut claude = app.api_client(&secret);

    let response = claude.post(&import, lot()).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    assert_eq!(
        response.json,
        json!({
            "ingredients_created": ["patate_douce"],
            "recipes_created": ["chili-con-carne", "dahl"],
            "recipes_updated": [],
            "recipe_warnings": [],
            "menu_imported": false,
            "warnings": [],
        })
    );
    let dahl = alice
        .get(&format!("/api/v1/households/{home}/recipes/dahl"))
        .await
        .json;
    assert_eq!(dahl["ingredients"][0]["name"], "Patate douce");

    // Republier le même lot : rien en double.
    let response = claude.post(&import, lot()).await;
    assert_eq!(
        response.json,
        json!({
            "ingredients_created": [],
            "recipes_created": [],
            "recipes_updated": ["chili-con-carne", "dahl"],
            "recipe_warnings": [],
            "menu_imported": false,
            "warnings": [],
        })
    );
    let list = alice
        .get(&format!("/api/v1/households/{home}/recipes"))
        .await
        .json;
    assert_eq!(list.as_array().unwrap().len(), 2);
}

#[sqlx::test]
async fn un_lot_invalide_n_ecrit_rien(db: PgPool) {
    ciqual::sync(&db).await.unwrap();
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = alice_and_home(&app).await;
    let import = format!("/api/v1/households/{home}/import");

    let mut bad = lot();
    bad["recipes"][1]["ingredients"][1]["ingredient"] = json!("licorne");
    let response = alice.post(&import, bad).await;
    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(response.text.contains("dahl"), "{}", response.text);
    assert!(response.text.contains("licorne"), "{}", response.text);
    let list = alice
        .get(&format!("/api/v1/households/{home}/recipes"))
        .await
        .json;
    assert_eq!(list, json!([]));
    let ingredients = alice.get("/api/v1/ingredients").await.json;
    assert!(
        ingredients
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["code"] != "patate_douce")
    );

    let mut no_slug = lot();
    no_slug["recipes"][0]
        .as_object_mut()
        .unwrap()
        .remove("slug");
    assert_eq!(
        alice.post(&import, no_slug).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let reader = token(&mut alice, home, json!(["read"])).await;
    assert_eq!(
        app.api_client(&reader).post(&import, lot()).await.status,
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test]
async fn le_catalogue_a_un_blender(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = alice_and_home(&app).await;
    let catalog = alice.get("/api/v1/equipment").await.json;
    assert!(
        catalog
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["code"] == "blender" && e["name_key"] == "equipment.blender")
    );
    let mut gaspacho = chili();
    gaspacho["variants"][0]["equipment"] = json!(["blender"]);
    let response = alice
        .put(
            &format!("/api/v1/households/{home}/recipes/gaspacho"),
            gaspacho,
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
}
