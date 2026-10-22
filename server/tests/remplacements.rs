//! Remplacements d'ingrédients : « lieu noir pour cabillaud, × 1,1 », saisis par l'admin ou issus d'une variante
//! tentée validée ; une recette les précise ou les refuse. Sur la liste de courses, choisir un remplacement change la
//! ligne et le plat (la cuisson), pour ce menu ; l'opération se rejoue hors ligne comme une case cochée.

mod common;

use axum::http::StatusCode;
use chaudron_server::config::SignupPolicy;
use common::{Browser, TestApp, config};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const ADMIN: &str = "admin@example.org";

async fn app(db: PgPool) -> TestApp {
    TestApp::with(db, |base| {
        let mut config = config(base, SignupPolicy::Open);
        config.admin_emails = vec![ADMIN.into()];
        config
    })
    .await
}

/// Alice, un adulte au foyer, un plat de cabillaud au menu du 2 novembre ; l'admin, et le lieu noir au référentiel.
async fn setup(app: &TestApp) -> (Browser, Uuid, Browser) {
    let mut admin = app.browser();
    admin.login(ADMIN).await;
    let created = admin
        .post(
            "/api/v1/ingredients",
            json!({"name": "Lieu noir", "aisle": "fish", "default_unit": "g", "default_state": "fresh"}),
        )
        .await;
    assert!(created.status.is_success(), "{}", created.text);
    let mut alice = app.browser();
    alice.login("alice@example.org").await;
    let home = alice.household("Maison").await;
    alice
        .post(
            &format!("/api/v1/households/{home}/members"),
            json!({"name": "Alice", "kind": "adult"}),
        )
        .await;
    let recipe = json!({
        "title": "Cabillaud vapeur",
        "ingredients": [{"ingredient": "cod", "qty": 150, "unit": "g", "role": "protein"},
                        {"ingredient": "potato", "qty": 200, "unit": "g", "role": "starch"}],
        "variants": [{"label": "Vapeur", "active_min": 10, "total_min": 25,
                      "phases": [{"title": "Cuire", "text": "Vapeur 12 min."}]}],
    });
    alice
        .put(
            &format!("/api/v1/households/{home}/recipes/cabillaud"),
            recipe,
        )
        .await;
    let menu = alice
        .put(
            &format!("/api/v1/households/{home}/menus/2026-11-02"),
            json!({"slots": [{"date": "2026-11-02", "meal": "dinner", "recipe": "cabillaud"}]}),
        )
        .await;
    assert!(menu.status.is_success(), "{}", menu.text);
    (alice, home, admin)
}

async fn add_substitution(admin: &mut Browser) -> String {
    let created = admin
        .post(
            "/api/v1/community/substitutions",
            json!({"ingredient": "cod", "substitute": "lieu_noir", "factor": 1.1, "note": "aussi bon, un peu plus ferme"}),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text);
    created.json["id"].as_str().unwrap().to_owned()
}

fn cod(list: &Value) -> Value {
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["ingredient"] == "cod" || i["replaces"]["ingredient"] == "cod")
        .cloned()
        .unwrap()
}

fn op(kind: &str, key: &Value, substitution: Value) -> Value {
    json!({"op_id": Uuid::now_v7(), "at": "2026-11-01T10:00:00Z", "week_start": "2026-11-02", "kind": kind,
           "key": key, "substitution_id": substitution})
}

#[sqlx::test]
async fn remplacer_sur_la_liste_change_la_cuisson(db: PgPool) {
    let app = app(db).await;
    let (mut alice, home, mut admin) = setup(&app).await;
    // Seul l'admin saisit un remplacement.
    assert_eq!(
        alice
            .post(
                "/api/v1/community/substitutions",
                json!({"ingredient": "cod", "substitute": "lieu_noir", "factor": 1})
            )
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let id = add_substitution(&mut admin).await;
    let all = alice.get("/api/v1/substitutions").await.json;
    assert_eq!(all[0]["substitute_name"], "Lieu noir");

    let shopping = format!("/api/v1/households/{home}/menus/2026-11-02/shopping");
    let item = cod(&alice.get(&shopping).await.json);
    assert_eq!(item["substitutes"][0]["id"], id.as_str());
    assert_eq!(item["substitutes"][0]["label"], "Lieu noir");
    assert_eq!(item["substitutes"][0]["qty"], 165.0);

    let ops = format!("/api/v1/households/{home}/shopping/ops");
    let choose = op("substitute", &item["key"], json!(id));
    let applied = alice.post(&ops, json!({"ops": [choose.clone()]})).await;
    assert_eq!(applied.status, StatusCode::OK, "{}", applied.text);
    assert_eq!(applied.json["applied"], 1);
    // Rejouée (hors ligne, puis de retour) : rien de plus.
    assert_eq!(
        alice.post(&ops, json!({"ops": [choose]})).await.json["applied"],
        0
    );

    let item = cod(&alice.get(&shopping).await.json);
    assert_eq!(item["label"], "Lieu noir");
    assert_eq!(item["qty"], 165.0);
    assert_eq!(item["substitution"], id.as_str());
    assert_eq!(item["replaces"]["label"], "Cabillaud");
    assert_eq!(item["replaces"]["qty"], 150.0);

    // Le plat en tient compte : la cuisson du créneau dit le lieu noir, et la précision.
    let cooking = alice
        .put(
            &format!("/api/v1/households/{home}/cooking"),
            json!({"recipe": "cabillaud", "date": "2026-11-02", "meal": "dinner", "mode": "fast", "phase": 0}),
        )
        .await
        .json;
    let line = &cooking["ingredients"][0];
    assert_eq!(line["name"], "Lieu noir");
    assert_eq!(line["qty"], 165.0);
    assert_eq!(line["replaces"], "Cabillaud");
    assert_eq!(line["note"], "aussi bon, un peu plus ferme");

    // Revenir au cabillaud.
    alice
        .post(
            &ops,
            json!({"ops": [op("substitute", &item["key"], Value::Null)]}),
        )
        .await;
    let item = cod(&alice.get(&shopping).await.json);
    assert_eq!(item["label"], "Cabillaud");
    assert_eq!(item["substitution"], Value::Null);
}

#[sqlx::test]
async fn remplacement_precise_ou_refuse_par_la_recette(db: PgPool) {
    let app = app(db).await;
    let (mut alice, home, mut admin) = setup(&app).await;
    let id = add_substitution(&mut admin).await;
    let rule = format!("/api/v1/households/{home}/recipes/cabillaud/substitutions/{id}");

    let precise = alice
        .put(&rule, json!({"allowed": true, "note": "+2 min de vapeur"}))
        .await;
    assert_eq!(precise.status, StatusCode::OK, "{}", precise.text);
    assert_eq!(precise.json["substitutions"][0]["note"], "+2 min de vapeur");
    let shopping = format!("/api/v1/households/{home}/menus/2026-11-02/shopping");
    let item = cod(&alice.get(&shopping).await.json);
    assert_eq!(item["substitutes"][0]["note"], "+2 min de vapeur");

    let refused = alice
        .put(&rule, json!({"allowed": false, "note": ""}))
        .await;
    assert_eq!(refused.status, StatusCode::OK);
    let item = cod(&alice.get(&shopping).await.json);
    assert_eq!(item["substitutes"], json!([]));
    let ops = format!("/api/v1/households/{home}/shopping/ops");
    let response = alice
        .post(
            &ops,
            json!({"ops": [op("substitute", &item["key"], json!(id))]}),
        )
        .await;
    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);

    assert_eq!(alice.delete(&rule).await.status, StatusCode::NO_CONTENT);
    let item = cod(&alice.get(&shopping).await.json);
    assert_eq!(
        item["substitutes"][0]["note"],
        "aussi bon, un peu plus ferme"
    );
}

#[sqlx::test]
async fn variante_tentee_devient_un_remplacement(db: PgPool) {
    let app = app(db).await;
    let (mut alice, home, mut admin) = setup(&app).await;
    // Le cabillaud d'Alice passe à la communauté.
    let id = alice
        .post(
            &format!("/api/v1/households/{home}/recipes/cabillaud/submit"),
            json!({}),
        )
        .await
        .json["id"]
        .as_str()
        .unwrap()
        .to_owned();
    admin
        .post(
            &format!("/api/v1/community/submissions/{id}/accept"),
            json!({}),
        )
        .await;
    let tried = alice
        .post(
            &format!("/api/v1/households/{home}/recipes/cabillaud/feedback"),
            json!({"visibility": "community", "kind": "tried", "body": "Avec du lieu noir : super.",
                   "tried": {"replaced": "cod", "by": "lieu_noir", "verdict": "good"}}),
        )
        .await;
    assert_eq!(tried.status, StatusCode::CREATED, "{}", tried.text);
    let feedback = tried.json["id"].as_str().unwrap().to_owned();

    let created = admin
        .post(
            "/api/v1/community/substitutions",
            json!({"ingredient": "cod", "substitute": "lieu_noir", "factor": 1.0, "feedback_id": feedback}),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text);
    assert_eq!(created.json["source"], "feedback");
    let mine = alice
        .get(&format!(
            "/api/v1/households/{home}/recipes/cabillaud/feedback?closed=true"
        ))
        .await
        .json;
    assert_eq!(mine[0]["status"], "applied");
    let notes = alice.get("/api/v1/me/notifications").await.json;
    assert_eq!(notes[0]["kind"], "feedback_applied");
}
