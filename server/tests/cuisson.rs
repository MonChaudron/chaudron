//! Cuisson en cours du foyer : une par foyer, position absolue (recette, variante, créneau, mode, phase, geste),
//! partagée entre l'Echo Show et le téléphone. La réponse porte l'étape à lire et les quantités du créneau.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use chaudron_server::config::SignupPolicy;
use common::{Browser, TestApp};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// Deux adultes et deux ados (à la maison le mercredi et le samedi midi). Le chili, en deux variantes : la cocotte
/// (trois phases, la deuxième en trois gestes) et le Cookeo (une phase).
async fn home(app: &TestApp) -> (Browser, Uuid) {
    let mut alice = app.browser();
    alice.login("alice@example.org").await;
    let home = alice.household("Maison").await;
    let members = format!("/api/v1/households/{home}/members");
    for (name, kind) in [
        ("Alice", "adult"),
        ("Bob", "adult"),
        ("Léo", "teen"),
        ("Zoé", "teen"),
    ] {
        let mut member = alice
            .post(&members, json!({"name": name, "kind": kind}))
            .await
            .json;
        if kind == "teen" {
            member["presence"]["lunch"] = json!([false, false, true, false, false, true, false]);
            let url = format!("{members}/{}", member["id"].as_str().unwrap());
            assert_eq!(alice.put(&url, member).await.status, StatusCode::OK);
        }
    }
    let response = alice
        .put(
            &format!("/api/v1/households/{home}/recipes/chili"),
            chili(3),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    (alice, home)
}

/// Le chili, avec `phases` phases à la cocotte.
fn chili(phases: usize) -> Value {
    let all = json!([
        {"title": "Les légumes", "text": "Oignon émincé, revenir.", "spoken_text": "On émince l'oignon."},
        {
            "title": "La viande",
            "text": "Le bœuf, puis le riz.",
            "timer_s": 600,
            "steps": [
                {"text": "Le bœuf à feu vif."},
                {"text": "Écraser à la spatule.", "spoken_text": "On écrase bien.", "timer_s": 360},
                {"text": "Le riz à part.", "video_url": "https://example.org/riz.mp4"},
            ],
        },
        {"title": "Servir", "text": "Coriandre."},
    ]);
    json!({
        "title": "Chili",
        "ingredients": [
            {"ingredient": "minced_beef", "qty": 150, "unit": "g", "role": "protein"},
            {"ingredient": "rice", "qty": 50, "unit": "g", "role": "starch"},
            {"ingredient": "garlic", "qty": 1, "unit": "piece", "role": "other", "per_batch": true},
            {"ingredient": "coriander", "unit": "g", "role": "other", "optional": true},
        ],
        "variants": [
            {
                "label": "Cocotte",
                "active_min": 20,
                "total_min": 60,
                "phases": all.as_array().unwrap()[..phases],
            },
            {
                "label": "Cookeo",
                "equipment": ["cookeo_6l"],
                "active_min": 10,
                "total_min": 30,
                "phases": [{"title": "Sous pression", "text": "Tout dans la cuve, 8 minutes."}],
            },
        ],
    })
}

/// Le chili dimanche 18 au soir, avec les boîtes du lundi midi : 4 + 2 portions.
async fn menu(alice: &mut Browser, home: Uuid) {
    let response = alice
        .put(
            &format!("/api/v1/households/{home}/menus/2026-10-18"),
            json!({"slots": [
                {"date": "2026-10-18", "meal": "dinner", "recipe": "chili", "covers_next_lunch": true},
            ]}),
        )
        .await;
    assert!(response.status.is_success(), "{}", response.text);
}

fn line<'a>(cooking: &'a Value, code: &str) -> &'a Value {
    cooking["ingredients"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["ingredient"] == code)
        .unwrap_or_else(|| panic!("{code} absent de {cooking}"))
}

fn at(phase: i64, step: Option<i64>, mode: &str) -> Value {
    json!({
        "recipe": "chili", "date": "2026-10-18", "meal": "dinner", "mode": mode, "phase": phase, "step": step,
    })
}

#[sqlx::test]
async fn cuisson_du_creneau(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = home(&app).await;
    menu(&mut alice, home).await;
    let url = format!("/api/v1/households/{home}/cooking");

    let response = alice.get(&url).await;
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", response.text);

    // Mode rapide, phase 1 : l'étape à lire, la position, les quantités du créneau.
    let response = alice.put(&url, at(0, None, "fast")).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let cooking = response.json;
    assert_eq!(cooking["recipe"]["slug"], "chili");
    assert_eq!(cooking["variant"], "Cocotte");
    assert_eq!(cooking["date"], "2026-10-18");
    assert_eq!(cooking["meal"], "dinner");
    assert_eq!(cooking["servings"], 6);
    assert_eq!(cooking["mode"], "fast");
    assert_eq!(cooking["phase"], 0);
    assert_eq!(cooking["step"], Value::Null);
    assert_eq!(cooking["phase_count"], 3);
    assert_eq!(cooking["step_count"], 0);
    assert_eq!(cooking["current"]["title"], "Les légumes");
    assert_eq!(cooking["current"]["text"], "Oignon émincé, revenir.");
    assert_eq!(cooking["current"]["spoken_text"], "On émince l'oignon.");
    // Bœuf : 150 g × 6 ; riz : les ados doublent le féculent au dîner (2 + 4), boîtes des adultes (2).
    assert_eq!(line(&cooking, "minced_beef")["qty"], json!(900.0));
    assert_eq!(line(&cooking, "rice")["qty"], json!(400.0));
    assert_eq!(line(&cooking, "garlic")["qty"], json!(1.0));
    assert_eq!(line(&cooking, "coriander")["qty"], Value::Null);
    assert_eq!(line(&cooking, "coriander")["optional"], true);
    let started = cooking["started_at"].clone();

    // Relue telle quelle.
    let read = alice.get(&url).await;
    assert_eq!(read.status, StatusCode::OK);
    assert_eq!(read.json["current"], cooking["current"]);

    // « Détaille » en phase 2 : le deuxième geste, avec son minuteur ; même cuisson, même début.
    let cooking = alice.put(&url, at(1, Some(1), "guided")).await.json;
    assert_eq!(cooking["mode"], "guided");
    assert_eq!(cooking["phase"], 1);
    assert_eq!(cooking["step"], 1);
    assert_eq!(cooking["step_count"], 3);
    assert_eq!(cooking["current"]["title"], "La viande");
    assert_eq!(cooking["current"]["text"], "Écraser à la spatule.");
    assert_eq!(cooking["current"]["spoken_text"], "On écrase bien.");
    assert_eq!(cooking["current"]["timer_s"], 360);
    assert_eq!(cooking["started_at"], started);

    // Pas à pas sur une phase sans gestes : la phase elle-même. Au-delà de la fin : la dernière phase.
    let cooking = alice.put(&url, at(9, Some(4), "guided")).await.json;
    assert_eq!(cooking["phase"], 2);
    assert_eq!(cooking["step"], Value::Null);
    assert_eq!(cooking["current"]["title"], "Servir");
    // En mode rapide, pas de geste.
    let cooking = alice.put(&url, at(1, Some(2), "fast")).await.json;
    assert_eq!(cooking["step"], Value::Null);
    assert_eq!(cooking["current"]["timer_s"], 600);

    // Autre variante, nommée.
    let mut body = at(0, None, "fast");
    body["variant"] = json!("Cookeo");
    let cooking = alice.put(&url, body).await.json;
    assert_eq!(cooking["variant"], "Cookeo");
    assert_eq!(cooking["phase_count"], 1);
    assert_eq!(cooking["current"]["title"], "Sous pression");

    // Fin de la cuisson.
    assert_eq!(alice.delete(&url).await.status, StatusCode::NO_CONTENT);
    assert_eq!(alice.get(&url).await.status, StatusCode::NO_CONTENT);
    assert_eq!(alice.delete(&url).await.status, StatusCode::NO_CONTENT);
}

#[sqlx::test]
async fn republier_ne_perd_pas_la_place(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = home(&app).await;
    menu(&mut alice, home).await;
    let url = format!("/api/v1/households/{home}/cooking");
    assert_eq!(
        alice.put(&url, at(2, None, "fast")).await.status,
        StatusCode::OK
    );

    // Le menu republié recrée ses créneaux : la cuisson retrouve le sien par la date, le repas et la recette.
    menu(&mut alice, home).await;
    let cooking = alice.get(&url).await.json;
    assert_eq!(cooking["servings"], 6);
    assert_eq!(cooking["phase"], 2);

    // La recette republiée avec deux phases : la place est ramenée dans les bornes.
    let response = alice
        .put(
            &format!("/api/v1/households/{home}/recipes/chili"),
            chili(2),
        )
        .await;
    assert!(response.status.is_success(), "{}", response.text);
    let cooking = alice.get(&url).await.json;
    assert_eq!(cooking["variant"], "Cocotte");
    assert_eq!(cooking["phase"], 1);
    assert_eq!(cooking["phase_count"], 2);
    assert_eq!(cooking["current"]["title"], "La viande");
}

#[sqlx::test]
async fn hors_menu_et_erreurs(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Open).await;
    let (mut alice, home) = home(&app).await;
    let url = format!("/api/v1/households/{home}/cooking");

    // Sans créneau : les membres présents au dîner du jour (les quatre, tous les soirs).
    let response = alice
        .put(&url, json!({"recipe": "chili", "mode": "fast", "phase": 0}))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    assert_eq!(response.json["servings"], 4);
    assert_eq!(response.json["date"], Value::Null);
    assert_eq!(line(&response.json, "minced_beef")["qty"], json!(600.0));
    assert_eq!(line(&response.json, "rice")["qty"], json!(300.0));

    for (body, why) in [
        (
            json!({"recipe": "inconnue", "mode": "fast", "phase": 0}),
            "recette inconnue",
        ),
        (
            json!({"recipe": "chili", "variant": "Wok", "mode": "fast", "phase": 0}),
            "variante inconnue",
        ),
        (
            json!({"recipe": "chili", "date": "2026-10-18", "mode": "fast", "phase": 0}),
            "date sans repas",
        ),
        (
            json!({"recipe": "chili", "mode": "fast", "phase": -1}),
            "phase négative",
        ),
    ] {
        let response = alice.put(&url, body).await;
        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{why} : {}",
            response.text
        );
    }

    // Un autre foyer ne voit rien.
    let mut eve = app.browser();
    eve.login("eve@example.org").await;
    assert_eq!(eve.get(&url).await.status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn six_heures_sans_nouvelles(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Closed).await;
    let (mut alice, home) = home(&app).await;
    menu(&mut alice, home).await;
    let url = format!("/api/v1/households/{home}/cooking");
    alice.put(&url, at(1, None, "fast")).await;

    sqlx::query("UPDATE cooking_sessions SET updated_at = now() - interval '5 hours 50 minutes'")
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(alice.get(&url).await.status, StatusCode::OK);
    sqlx::query("UPDATE cooking_sessions SET updated_at = now() - interval '6 hours 10 minutes'")
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(alice.get(&url).await.status, StatusCode::NO_CONTENT);

    // Reprendre ensuite, c'est une nouvelle cuisson.
    let cooking = alice.put(&url, at(1, None, "fast")).await.json;
    let started: time::OffsetDateTime = time::OffsetDateTime::parse(
        cooking["started_at"].as_str().unwrap(),
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    assert!(time::OffsetDateTime::now_utc() - started < time::Duration::minutes(1));
}

#[sqlx::test]
async fn jetons_et_flux_sse(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = home(&app).await;
    menu(&mut alice, home).await;
    let url = format!("/api/v1/households/{home}/cooking");
    let tokens = format!("/api/v1/households/{home}/tokens");

    let reader = alice
        .post(&tokens, json!({"name": "Lecture", "scopes": ["read"]}))
        .await
        .json["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let response = alice
        .post(
            &tokens,
            json!({"name": "Alexa", "scopes": ["read", "cooking:write"]}),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    let cook = response.json["token"].as_str().unwrap().to_owned();

    let mut reader = app.api_client(&reader);
    assert_eq!(
        reader.put(&url, at(0, None, "fast")).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(reader.delete(&url).await.status, StatusCode::FORBIDDEN);

    let response = alice
        .open(&format!("/api/v1/households/{home}/events"))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();

    let mut cook = app.api_client(&cook);
    let response = cook.put(&url, at(1, None, "fast")).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let mut received = String::new();
    while !received.contains("event: cooking") {
        let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
            .await
            .expect("un événement dans les 5 s")
            .unwrap()
            .unwrap();
        if let Ok(data) = frame.into_data() {
            received.push_str(std::str::from_utf8(&data).unwrap());
        }
    }
    // Le téléphone lit la même place.
    assert_eq!(reader.get(&url).await.json["phase"], 1);
}
