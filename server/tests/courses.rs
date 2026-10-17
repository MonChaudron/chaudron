//! Liste de courses d'un menu : calculée depuis les créneaux, rangée par rayon ; cases cochées et ajouts à la main
//! par opérations (dernière écriture gagne, idempotentes) ; journal des opérations et flux SSE.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use chaudron_server::config::SignupPolicy;
use common::{Browser, TestApp};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// Deux adultes et deux ados (à la maison le mercredi et le samedi midi). Le chili et le saumon.
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
    let phases = json!([{"title": "Cuire", "text": "Cuire."}]);
    let chili = json!({
        "title": "Chili",
        "ingredients": [
            {"ingredient": "minced_beef", "qty": 150, "unit": "g", "role": "protein"},
            {"ingredient": "rice", "qty": 50, "unit": "g", "role": "starch"},
            {"ingredient": "onion", "qty": 0.5, "unit": "piece", "role": "veg"},
            {"ingredient": "garlic", "qty": 1, "unit": "piece", "role": "other", "per_batch": true},
            {"ingredient": "butter", "qty": 1, "unit": "tbsp", "role": "fat"},
            {"ingredient": "coriander", "unit": "g", "role": "other", "optional": true},
        ],
        "variants": [{"label": "Cocotte", "active_min": 20, "total_min": 60, "phases": phases}],
    });
    let saumon = json!({
        "title": "Saumon",
        "ingredients": [
            {"ingredient": "salmon", "qty": 150, "unit": "g", "role": "protein"},
            {"ingredient": "spinach", "state": "frozen", "qty": 100, "unit": "g", "role": "veg"},
            {"ingredient": "chickpea", "state": "canned", "qty": 50, "unit": "g", "role": "veg"},
            {"ingredient": "onion", "qty": 0.25, "unit": "piece", "role": "veg"},
        ],
        "variants": [{"label": "Plaque", "active_min": 10, "total_min": 20, "phases": phases}],
    });
    for (slug, recipe) in [("chili", chili), ("saumon", saumon)] {
        let response = alice
            .put(&format!("/api/v1/households/{home}/recipes/{slug}"), recipe)
            .await;
        assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    }
    (alice, home)
}

fn dinner(date: &str, recipe: &str, covers: bool) -> Value {
    json!({"date": date, "meal": "dinner", "recipe": recipe, "covers_next_lunch": covers})
}

/// Le menu de la semaine du 18 : chili dimanche (4 + 2) et mardi (4 + 4), saumon mercredi (4).
async fn menu(alice: &mut Browser, home: Uuid, extra: Option<Value>) {
    let mut slots = vec![
        dinner("2026-10-18", "chili", true),
        dinner("2026-10-20", "chili", true),
        dinner("2026-10-21", "saumon", false),
    ];
    slots.extend(extra);
    let response = alice
        .put(
            &format!("/api/v1/households/{home}/menus/2026-10-18"),
            json!({"slots": slots}),
        )
        .await;
    assert!(response.status.is_success(), "{}", response.text);
}

fn item<'a>(list: &'a Value, label: &str) -> &'a Value {
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["label"] == label)
        .unwrap_or_else(|| panic!("{label} absent de {list}"))
}

fn op(kind: &str, key: &str, at: &str) -> Value {
    json!({"op_id": Uuid::now_v7(), "at": at, "week_start": "2026-10-18", "kind": kind, "key": key})
}

#[sqlx::test]
async fn liste_calculee_par_rayon(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = home(&app).await;
    menu(&mut alice, home, None).await;

    let response = alice
        .get(&format!(
            "/api/v1/households/{home}/menus/2026-10-18/shopping"
        ))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let list = response.json;
    assert_eq!(list["week_start"], "2026-10-18");

    // Bœuf : 150 g × (6 + 8) portions.
    let beef = item(&list, "Bœuf haché");
    assert_eq!(beef["qty"], json!(2100.0));
    assert_eq!(beef["unit"], "g");
    assert_eq!(beef["aisle"], "butcher");
    assert_eq!(beef["block"], "recipes");
    assert_eq!(beef["recipes"], json!(["Chili"]));
    assert_eq!(beef["checked"], false);
    assert_eq!(beef["manual"], false);
    // Riz : les ados doublent le féculent (8 + 12 portions standard) ; sec, donc à l'épicerie.
    let rice = item(&list, "Riz");
    assert_eq!(rice["qty"], json!(1000.0));
    assert_eq!(rice["aisle"], "grocery");
    // Oignon : 0,5 × 14 au chili, 0,25 × 4 au saumon : 8 ; les pièces s'achètent entières.
    let onion = item(&list, "Oignon");
    assert_eq!(onion["qty"], json!(8.0));
    assert_eq!(onion["recipes"], json!(["Chili", "Saumon"]));
    // Une ligne par plat compte une fois par créneau.
    assert_eq!(item(&list, "Ail")["qty"], json!(2.0));
    // Surgelé et conserve changent de rayon.
    assert_eq!(item(&list, "Épinard")["aisle"], "frozen");
    assert_eq!(item(&list, "Épinard")["qty"], json!(400.0));
    assert_eq!(item(&list, "Pois chiches")["aisle"], "grocery");
    // Cuillères et « au goût » : le placard, à vérifier.
    let butter = item(&list, "Beurre");
    assert_eq!(butter["pantry"], true);
    assert_eq!(butter["qty"], json!(14.0));
    let coriander = item(&list, "Coriandre");
    assert_eq!(coriander["pantry"], true);
    assert_eq!(coriander["qty"], Value::Null);
    assert_eq!(coriander["optional"], true);
    assert_eq!(beef["pantry"], false);

    // Ordre du magasin : fruits et légumes d'abord, surgelés en dernier, le placard après les rayons.
    let labels: Vec<&str> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["label"].as_str().unwrap())
        .collect();
    assert_eq!(
        labels,
        [
            "Ail",
            "Oignon",
            "Bœuf haché",
            "Saumon",
            "Pois chiches",
            "Riz",
            "Épinard",
            "Beurre",
            "Coriandre"
        ]
    );
}

#[sqlx::test]
async fn cocher_garde_la_case(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = home(&app).await;
    menu(&mut alice, home, None).await;
    let shopping = format!("/api/v1/households/{home}/menus/2026-10-18/shopping");
    let ops = format!("/api/v1/households/{home}/shopping/ops");
    let onion = item(&alice.get(&shopping).await.json, "Oignon")["key"]
        .as_str()
        .unwrap()
        .to_owned();

    let check = op("check", &onion, "2026-10-17T10:00:00Z");
    let response = alice.post(&ops, json!({"ops": [check.clone()]})).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    assert_eq!(response.json["applied"], 1);
    let cursor = response.json["cursor"].as_i64().unwrap();
    assert!(cursor > 0);
    let list = alice.get(&shopping).await.json;
    let checked = item(&list, "Oignon");
    assert_eq!(checked["checked"], true);
    assert_eq!(checked["checked_at"], "2026-10-17T10:00:00Z");
    assert!(checked["checked_by"].is_string());
    assert_eq!(checked["grown"], Value::Null);
    assert_eq!(list["cursor"], cursor);

    // La même opération rejouée (file hors ligne) : rien de plus.
    let response = alice.post(&ops, json!({"ops": [check]})).await;
    assert_eq!(response.json["applied"], 0);
    // Une opération plus ancienne arrive après : la dernière écriture gagne.
    let response = alice
        .post(
            &ops,
            json!({"ops": [op("uncheck", &onion, "2026-10-17T09:00:00Z")]}),
        )
        .await;
    assert_eq!(response.json["applied"], 0);
    assert_eq!(
        item(&alice.get(&shopping).await.json, "Oignon")["checked"],
        true
    );

    // Le menu change (un chili de plus jeudi, 4 + 2) : la case reste, la hausse est signalée.
    menu(&mut alice, home, Some(dinner("2026-10-22", "chili", true))).await;
    let onion_now = item(&alice.get(&shopping).await.json, "Oignon").clone();
    assert_eq!(onion_now["qty"], json!(11.0));
    assert_eq!(onion_now["checked"], true);
    assert_eq!(onion_now["grown"], json!(3.0));

    // Décocher.
    alice
        .post(
            &ops,
            json!({"ops": [op("uncheck", &onion, "2026-10-17T11:00:00Z")]}),
        )
        .await;
    assert_eq!(
        item(&alice.get(&shopping).await.json, "Oignon")["checked"],
        false
    );

    // Le journal, depuis un curseur.
    let response = alice.get(&format!("{ops}?since=0")).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let journal = response.json["ops"].as_array().unwrap();
    assert_eq!(journal.len(), 2);
    assert_eq!(journal[0]["kind"], "check");
    assert_eq!(journal[1]["kind"], "uncheck");
    let response = alice
        .get(&format!("{ops}?since={}", journal[0]["seq"]))
        .await;
    assert_eq!(response.json["ops"].as_array().unwrap().len(), 1);

    for wrong in [
        op("check", "inconnue", "2026-10-17T12:00:00Z"),
        json!({"op_id": Uuid::now_v7(), "at": "2026-10-17T12:00:00Z", "week_start": "2026-10-11", "kind": "check", "key": onion}),
    ] {
        let response = alice.post(&ops, json!({"ops": [wrong]})).await;
        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{}",
            response.text
        );
    }
}

#[sqlx::test]
async fn ajouts_a_la_main(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = home(&app).await;
    menu(&mut alice, home, None).await;
    let shopping = format!("/api/v1/households/{home}/menus/2026-10-18/shopping");
    let ops = format!("/api/v1/households/{home}/shopping/ops");
    let key = format!("manual:{}", Uuid::now_v7());

    let mut add = op("add", &key, "2026-10-17T10:00:00Z");
    add["label"] = json!("Yaourts");
    add["qty"] = json!(8);
    add["unit"] = json!("piece");
    add["aisle"] = json!("dairy");
    add["block"] = json!("breakfast_desserts");
    let response = alice.post(&ops, json!({"ops": [add]})).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let list = alice.get(&shopping).await.json;
    let yogurt = item(&list, "Yaourts");
    assert_eq!(yogurt["manual"], true);
    assert_eq!(yogurt["block"], "breakfast_desserts");
    assert_eq!(yogurt["qty"], json!(8.0));
    // Les blocs viennent après les recettes.
    assert_eq!(
        list["items"].as_array().unwrap().last().unwrap()["label"],
        "Yaourts"
    );

    let mut edit = op("edit", &key, "2026-10-17T10:01:00Z");
    edit["label"] = json!("Yaourts nature");
    edit["qty"] = json!(12);
    alice
        .post(
            &ops,
            json!({"ops": [edit, op("check", &key, "2026-10-17T10:02:00Z")]}),
        )
        .await;
    let list = alice.get(&shopping).await.json;
    let yogurt = item(&list, "Yaourts nature");
    assert_eq!(yogurt["qty"], json!(12.0));
    assert_eq!(yogurt["unit"], "piece");
    assert_eq!(yogurt["checked"], true);

    alice
        .post(
            &ops,
            json!({"ops": [op("remove", &key, "2026-10-17T10:03:00Z")]}),
        )
        .await;
    let list = alice.get(&shopping).await.json;
    assert!(
        !list["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["manual"] == true)
    );
    // Un article calculé ne se retire pas.
    let beef = item(&list, "Bœuf haché")["key"]
        .as_str()
        .unwrap()
        .to_owned();
    let response = alice
        .post(
            &ops,
            json!({"ops": [op("remove", &beef, "2026-10-17T10:04:00Z")]}),
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
async fn flux_sse(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = home(&app).await;
    menu(&mut alice, home, None).await;
    let onion = item(
        &alice
            .get(&format!(
                "/api/v1/households/{home}/menus/2026-10-18/shopping"
            ))
            .await
            .json,
        "Oignon",
    )["key"]
        .as_str()
        .unwrap()
        .to_owned();

    let response = alice
        .open(&format!("/api/v1/households/{home}/events"))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let mut body = response.into_body();

    alice
        .post(
            &format!("/api/v1/households/{home}/shopping/ops"),
            json!({"ops": [op("check", &onion, "2026-10-17T10:00:00Z")]}),
        )
        .await;
    menu(&mut alice, home, None).await;
    let mut received = String::new();
    while !(received.contains("event: shopping") && received.contains("event: menu")) {
        let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
            .await
            .expect("un événement dans les 5 s")
            .unwrap()
            .unwrap();
        if let Ok(data) = frame.into_data() {
            received.push_str(std::str::from_utf8(&data).unwrap());
        }
    }
    assert!(
        received.contains(r#""week_start":"2026-10-18""#),
        "{received}"
    );
}

#[sqlx::test]
async fn courses_d_un_autre_foyer_invisibles(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Open).await;
    let (mut alice, home) = home(&app).await;
    menu(&mut alice, home, None).await;
    let shopping = format!("/api/v1/households/{home}/menus/2026-10-18/shopping");

    let mut eve = app.browser();
    eve.login("eve@example.org").await;
    assert_eq!(eve.get(&shopping).await.status, StatusCode::NOT_FOUND);
    let response = eve
        .post(
            &format!("/api/v1/households/{home}/shopping/ops"),
            json!({"ops": []}),
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert_eq!(
        eve.open(&format!("/api/v1/households/{home}/events"))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        alice
            .get(&format!(
                "/api/v1/households/{home}/menus/2026-11-01/shopping"
            ))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}
