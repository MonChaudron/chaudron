//! Le cahier du foyer : notes privées sur une recette ou une étape (du foyer ou de la communauté), « Alexa,
//! remarque » à l'étape en cours, notes lues avec la cuisson, « comment c'était ? ».

mod common;

use axum::http::StatusCode;
use chaudron_server::config::SignupPolicy;
use common::{Browser, TestApp, link_alexa};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

async fn home_of(app: &TestApp, email: &str, name: &str) -> (Browser, Uuid) {
    let mut browser = app.browser();
    browser.login(email).await;
    let home = browser.household(name).await;
    (browser, home)
}

fn gratin() -> Value {
    json!({
        "title": "Gratin dauphinois",
        "ingredients": [
            {"ingredient": "potato", "qty": 250, "unit": "g", "role": "starch"},
            {"ingredient": "cream", "qty": 50, "unit": "ml", "role": "fat"},
        ],
        "variants": [{
            "label": "Four",
            "equipment": ["oven"],
            "active_min": 20,
            "total_min": 80,
            "phases": [
                {"title": "Les pommes de terre", "text": "Éplucher, trancher à 3 mm.",
                 "steps": [{"text": "Éplucher."}, {"text": "Trancher à 3 mm."}]},
                {"title": "Cuire", "text": "Four à 160 °C, 1 h.", "timer_s": 3600},
            ],
        }],
    })
}

/// Le gratin publié chez ce foyer : la recette lue (avec ses clés).
async fn publish(browser: &mut Browser, home: Uuid) -> Value {
    let response = browser
        .put(
            &format!("/api/v1/households/{home}/recipes/gratin"),
            gratin(),
        )
        .await;
    assert!(response.status.is_success(), "{}", response.text);
    response.json
}

#[sqlx::test]
async fn note_d_etape_lue_par_la_cuisson(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Open).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    let recipe = publish(&mut alice, home).await;
    let cuire = recipe["variants"][0]["phases"][1]["key"].clone();
    let feedback = format!("/api/v1/households/{home}/recipes/gratin/feedback");

    let note = alice
        .post(
            &feedback,
            json!({"element_key": cuire, "body": "Chez nous, 1 h 10 : le cœur restait ferme.",
                   "changes": {"timer_s": 4200}}),
        )
        .await;
    assert_eq!(note.status, StatusCode::CREATED, "{}", note.text);
    assert_eq!(note.json["visibility"], "household");
    assert_eq!(note.json["status"], "open");
    assert_eq!(note.json["author_name"], "alice");
    assert_eq!(
        note.json["changes"],
        json!({"timer_s": {"from": 3600, "to": 4200}})
    );
    let general = alice
        .post(&feedback, json!({"body": "Les enfants adorent."}))
        .await;
    assert_eq!(general.status, StatusCode::CREATED, "{}", general.text);

    // Dans la recette lue (et donc l'agenda, la copie de travail du téléphone).
    let read = alice
        .get(&format!("/api/v1/households/{home}/recipes/gratin"))
        .await
        .json;
    assert_eq!(read["notes"].as_array().unwrap().len(), 2);
    assert_eq!(read["notes"][0]["element_key"], cuire);

    // La cuisson donne les notes de l'étape à l'écran, et celles de la recette entière.
    let cooking = alice
        .put(
            &format!("/api/v1/households/{home}/cooking"),
            json!({"recipe": "gratin", "mode": "fast", "phase": 1}),
        )
        .await;
    assert_eq!(cooking.status, StatusCode::OK, "{}", cooking.text);
    assert_eq!(
        cooking.json["step_notes"][0]["body"],
        "Chez nous, 1 h 10 : le cœur restait ferme."
    );
    assert_eq!(
        cooking.json["recipe_notes"][0]["body"],
        "Les enfants adorent."
    );
    let first = alice
        .put(
            &format!("/api/v1/households/{home}/cooking"),
            json!({"recipe": "gratin", "mode": "fast", "phase": 0}),
        )
        .await;
    assert_eq!(first.json["step_notes"], json!([]));

    // Un changement ne vaut que pour une étape, un geste ou une ligne, et sur un champ qu'il connaît.
    let bad = alice
        .post(&feedback, json!({"body": "x", "changes": {"timer_s": 60}}))
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let bad = alice
        .post(
            &feedback,
            json!({"element_key": cuire, "body": "x", "changes": {"title": "Autre"}}),
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let bad = alice
        .post(
            &feedback,
            json!({"element_key": Uuid::now_v7(), "body": "x"}),
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test]
async fn notes_invisibles_des_autres_foyers(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Open).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    publish(&mut alice, home).await;
    sqlx::query("UPDATE recipes SET household_id = NULL")
        .execute(&db)
        .await
        .unwrap();
    let note = alice
        .post(
            &format!("/api/v1/households/{home}/recipes/gratin/feedback"),
            json!({"body": "Notre façon : moins de crème."}),
        )
        .await;
    assert_eq!(note.status, StatusCode::CREATED, "{}", note.text);
    let id = note.json["id"].as_str().unwrap().to_owned();

    let (mut bob, bob_home) = home_of(&app, "bob@example.org", "Chez Bob").await;
    let read = bob
        .get(&format!("/api/v1/households/{bob_home}/recipes/gratin"))
        .await
        .json;
    assert_eq!(read["notes"], json!([]));
    let list = bob
        .get(&format!(
            "/api/v1/households/{bob_home}/recipes/gratin/feedback?closed=true"
        ))
        .await
        .json;
    assert_eq!(list, json!([]));
    let patch = bob
        .patch(
            &format!("/api/v1/households/{bob_home}/feedback/{id}"),
            json!({"closed": true}),
        )
        .await;
    assert_eq!(patch.status, StatusCode::NOT_FOUND);
    let patch = bob
        .patch(
            &format!("/api/v1/households/{home}/feedback/{id}"),
            json!({"closed": true}),
        )
        .await;
    assert_eq!(patch.status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn fermer_rouvrir_et_supprimer_une_note(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Open).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    publish(&mut alice, home).await;
    let feedback = format!("/api/v1/households/{home}/recipes/gratin/feedback");
    let id = alice
        .post(&feedback, json!({"body": "Trop salé."}))
        .await
        .json["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let note = format!("/api/v1/households/{home}/feedback/{id}");

    let closed = alice.patch(&note, json!({"closed": true})).await;
    assert_eq!(closed.status, StatusCode::OK, "{}", closed.text);
    assert_eq!(closed.json["status"], "closed");
    let read = alice
        .get(&format!("/api/v1/households/{home}/recipes/gratin"))
        .await
        .json;
    assert_eq!(read["notes"], json!([]));
    let all = alice.get(&format!("{feedback}?closed=true")).await.json;
    assert_eq!(all[0]["status"], "closed");
    let open = alice.get(&feedback).await.json;
    assert_eq!(open, json!([]));

    let edited = alice
        .patch(&note, json!({"closed": false, "body": "Un peu salé."}))
        .await;
    assert_eq!(edited.json["status"], "open");
    assert_eq!(edited.json["body"], "Un peu salé.");
    assert_eq!(alice.delete(&note).await.status, StatusCode::NO_CONTENT);
    assert_eq!(alice.get(&feedback).await.json, json!([]));
}

#[sqlx::test]
async fn une_note_survit_a_la_republication_de_claude(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Open).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    let recipe = publish(&mut alice, home).await;
    let cuire = recipe["variants"][0]["phases"][1]["key"].clone();
    let feedback = format!("/api/v1/households/{home}/recipes/gratin/feedback");
    alice
        .post(
            &feedback,
            json!({"element_key": cuire, "body": "1 h 10 chez nous."}),
        )
        .await;

    // Republiée sans clés : la note reste sur son étape.
    let mut again = gratin();
    again["variants"][0]["phases"][0]["text"] = json!("Éplucher, trancher à 2 mm.");
    alice
        .put(&format!("/api/v1/households/{home}/recipes/gratin"), again)
        .await;
    let read = alice
        .get(&format!("/api/v1/households/{home}/recipes/gratin"))
        .await
        .json;
    assert_eq!(read["notes"][0]["element_key"], cuire);

    // L'étape disparaît : la note remonte sur la recette, elle ne se perd pas.
    let mut shorter = gratin();
    shorter["variants"][0]["phases"]
        .as_array_mut()
        .unwrap()
        .pop();
    alice
        .put(
            &format!("/api/v1/households/{home}/recipes/gratin"),
            shorter,
        )
        .await;
    let read = alice
        .get(&format!("/api/v1/households/{home}/recipes/gratin"))
        .await
        .json;
    assert_eq!(read["notes"][0]["element_key"], Value::Null);
    assert_eq!(read["notes"][0]["body"], "1 h 10 chez nous.");
}

#[sqlx::test]
async fn remarque_d_alexa_a_l_etape_en_cours(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Open).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    let recipe = publish(&mut alice, home).await;
    let token = link_alexa(&app, &mut alice, home).await;
    let mut alexa = app.api_client(&token);
    let remark = format!("/api/v1/households/{home}/cooking/remark");

    // Sans cuisson en cours, rien à quoi l'attacher.
    let none = alexa.post(&remark, json!({"text": "ça rentre pas"})).await;
    assert_eq!(none.status, StatusCode::CONFLICT);
    assert_eq!(none.json["error"], "no_cooking");

    alexa
        .put(
            &format!("/api/v1/households/{home}/cooking"),
            json!({"recipe": "gratin", "mode": "guided", "phase": 0, "step": 1}),
        )
        .await;
    let response = alexa
        .post(&remark, json!({"text": "  ça rentre pas dans le plat  "}))
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    // Au pas à pas, la remarque va au geste à l'écran ; signée du compte qui a lié Alexa.
    assert_eq!(
        response.json["element_key"],
        recipe["variants"][0]["phases"][0]["steps"][1]["key"]
    );
    assert_eq!(response.json["body"], "ça rentre pas dans le plat");
    assert_eq!(response.json["author_name"], "alice");
    assert_eq!(response.json["visibility"], "household");
    let cooking = alexa
        .get(&format!("/api/v1/households/{home}/cooking"))
        .await
        .json;
    assert_eq!(
        cooking["step_notes"][0]["body"],
        "ça rentre pas dans le plat"
    );
    assert_eq!(
        alexa.post(&remark, json!({"text": " "})).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[sqlx::test]
async fn comment_c_etait(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Open).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    publish(&mut alice, home).await;
    alice
        .put(
            &format!("/api/v1/households/{home}/menus/2026-10-19"),
            json!({"slots": [{"date": "2026-10-19", "meal": "dinner", "recipe": "gratin"},
                             {"date": "2026-10-21", "meal": "dinner", "recipe": "gratin"}]}),
        )
        .await;
    let ratings = format!("/api/v1/households/{home}/ratings");
    let rated = alice
        .put(
            &ratings,
            json!({"recipe": "gratin", "date": "2026-10-19", "meal": "dinner", "rating": "top",
                   "word": "Super avec du comté."}),
        )
        .await;
    assert_eq!(rated.status, StatusCode::NO_CONTENT, "{}", rated.text);
    // Changer d'avis remplace la note du repas.
    alice
        .put(
            &ratings,
            json!({"recipe": "gratin", "date": "2026-10-21", "meal": "dinner", "rating": "bof"}),
        )
        .await;
    alice
        .put(
            &ratings,
            json!({"recipe": "gratin", "date": "2026-10-21", "meal": "dinner", "rating": "flop"}),
        )
        .await;

    let menu = alice
        .get(&format!("/api/v1/households/{home}/menus/2026-10-19"))
        .await
        .json;
    assert_eq!(menu["slots"][0]["rating"], "top");
    assert_eq!(menu["slots"][1]["rating"], "flop");
    let recipe = alice
        .get(&format!("/api/v1/households/{home}/recipes/gratin"))
        .await
        .json;
    assert_eq!(recipe["ratings"], json!({"top": 1, "bof": 0, "flop": 1}));
    // Le mot devient une note de la recette.
    assert_eq!(recipe["notes"][0]["body"], "Super avec du comté.");

    let unknown = alice
        .put(
            &ratings,
            json!({"recipe": "inconnue", "date": "2026-10-19", "meal": "dinner", "rating": "top"}),
        )
        .await;
    assert_eq!(unknown.status, StatusCode::UNPROCESSABLE_ENTITY);
}
