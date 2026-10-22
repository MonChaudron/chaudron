//! Proposer une recette du foyer à la communauté (l'admin accepte ou refuse, le foyer est prévenu), copier une recette
//! de la communauté chez soi et reprendre les changements de l'originale.

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

async fn home_of(app: &TestApp, email: &str, name: &str) -> (Browser, Uuid) {
    let mut browser = app.browser();
    browser.login(email).await;
    let home = browser.household(name).await;
    (browser, home)
}

fn gratin(title: &str) -> Value {
    json!({
        "title": title,
        "ingredients": [{"ingredient": "potato", "qty": 250, "unit": "g", "role": "starch"}],
        "variants": [{
            "label": "Four",
            "active_min": 20,
            "total_min": 80,
            "phases": [
                {"title": "Trancher", "text": "Trancher à 3 mm."},
                {"title": "Cuire", "text": "Four à 160 °C, 1 h.", "timer_s": 3600},
            ],
        }],
    })
}

async fn admin(app: &TestApp) -> Browser {
    let mut admin = app.browser();
    admin.login(ADMIN).await;
    admin
}

/// Alice propose son gratin ; renvoie l'identifiant de la proposition.
async fn submit(alice: &mut Browser, home: Uuid) -> String {
    let response = alice
        .post(
            &format!("/api/v1/households/{home}/recipes/gratin/submit"),
            json!({"message": "Notre gratin du dimanche, testé dix fois."}),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    assert_eq!(response.json["status"], "pending");
    response.json["id"].as_str().unwrap().to_owned()
}

#[sqlx::test]
async fn accepter_fait_passer_la_recette_a_la_communaute_sans_toucher_les_menus(db: PgPool) {
    let app = app(db).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    alice
        .put(
            &format!("/api/v1/households/{home}/recipes/gratin"),
            gratin("Gratin dauphinois"),
        )
        .await;
    alice
        .put(
            &format!("/api/v1/households/{home}/menus/2026-11-02"),
            json!({"slots": [{"date": "2026-11-02", "meal": "dinner", "recipe": "gratin"}]}),
        )
        .await;
    let note = alice
        .post(
            &format!("/api/v1/households/{home}/recipes/gratin/feedback"),
            json!({"body": "Chez nous, plus d'ail."}),
        )
        .await;
    assert_eq!(note.status, StatusCode::CREATED);
    let id = submit(&mut alice, home).await;
    // Une seule proposition en attente par recette.
    let again = alice
        .post(
            &format!("/api/v1/households/{home}/recipes/gratin/submit"),
            json!({}),
        )
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    let read = alice
        .get(&format!("/api/v1/households/{home}/recipes/gratin"))
        .await
        .json;
    assert_eq!(read["submission"]["status"], "pending");

    // Personne d'autre que l'admin ne voit les propositions.
    assert_eq!(
        alice.get("/api/v1/community/review").await.status,
        StatusCode::FORBIDDEN
    );
    let mut admin = admin(&app).await;
    let review = admin.get("/api/v1/community/review").await;
    assert_eq!(review.status, StatusCode::OK, "{}", review.text);
    assert_eq!(review.json["submissions"][0]["title"], "Gratin dauphinois");
    assert_eq!(review.json["submissions"][0]["household_name"], "Maison");
    let seen = admin
        .get(&format!("/api/v1/community/submissions/{id}"))
        .await;
    assert_eq!(seen.status, StatusCode::OK, "{}", seen.text);
    assert_eq!(
        seen.json["recipe"]["variants"][0]["phases"][1]["title"],
        "Cuire"
    );

    let accepted = admin
        .post(
            &format!("/api/v1/community/submissions/{id}/accept"),
            json!({"message": "Merci !"}),
        )
        .await;
    assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.text);
    assert_eq!(accepted.json["community"], true);
    assert_eq!(accepted.json["slug"], "gratin");

    // Même recette, même adresse : le menu d'Alice n'a pas bougé, sa note non plus.
    let recipe = alice
        .get(&format!("/api/v1/households/{home}/recipes/gratin"))
        .await
        .json;
    assert_eq!(recipe["community"], true);
    assert_eq!(recipe["notes"][0]["body"], "Chez nous, plus d'ail.");
    let menu = alice
        .get(&format!("/api/v1/households/{home}/menus/2026-11-02"))
        .await
        .json;
    assert_eq!(menu["slots"][0]["recipe"]["slug"], "gratin");
    let versions = alice
        .get(&format!(
            "/api/v1/households/{home}/recipes/gratin/versions"
        ))
        .await
        .json;
    assert_eq!(versions[0]["message"], "Acceptée dans la communauté");
    // Alice ne la modifie plus directement.
    assert_eq!(
        alice
            .put(
                &format!("/api/v1/households/{home}/recipes/gratin"),
                gratin("Autre")
            )
            .await
            .status,
        StatusCode::CONFLICT
    );

    // Alice est prévenue.
    let me = alice.me().await;
    assert_eq!(me["unread_notifications"], 1);
    let notes = alice.get("/api/v1/me/notifications").await.json;
    assert_eq!(notes[0]["kind"], "submission_accepted");
    assert!(
        notes[0]["text"]
            .as_str()
            .unwrap()
            .contains("Gratin dauphinois")
    );
    assert_eq!(notes[0]["recipe_slug"], "gratin");
    assert_eq!(
        alice
            .post("/api/v1/me/notifications/read", json!({}))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(alice.me().await["unread_notifications"], 0);
}

#[sqlx::test]
async fn accepter_un_slug_pris_ailleurs_et_modifier_en_acceptant(db: PgPool) {
    let app = app(db).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    alice
        .put(
            &format!("/api/v1/households/{home}/recipes/gratin"),
            gratin("Gratin dauphinois"),
        )
        .await;
    let (mut bob, bob_home) = home_of(&app, "bob@example.org", "Chez Bob").await;
    bob.put(
        &format!("/api/v1/households/{bob_home}/recipes/gratin"),
        gratin("Le gratin de Bob"),
    )
    .await;
    let id = submit(&mut alice, home).await;
    let mut admin = admin(&app).await;

    // Bob a déjà un « gratin » : la communauté ne peut pas le prendre.
    let taken = admin
        .post(
            &format!("/api/v1/community/submissions/{id}/accept"),
            json!({}),
        )
        .await;
    assert_eq!(taken.status, StatusCode::CONFLICT, "{}", taken.text);
    assert_eq!(taken.json["error"], "slug_taken");

    let mut edited = gratin("Gratin dauphinois");
    edited["variants"][0]["phases"][0]["text"] = json!("Trancher à 2 mm, pas plus.");
    let accepted = admin
        .post(
            &format!("/api/v1/community/submissions/{id}/accept"),
            json!({"slug": "gratin-dauphinois", "recipe": edited}),
        )
        .await;
    assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.text);
    assert_eq!(accepted.json["slug"], "gratin-dauphinois");
    assert_eq!(
        accepted.json["variants"][0]["phases"][0]["text"],
        "Trancher à 2 mm, pas plus."
    );
    // Les deux foyers la voient ; Bob garde son gratin.
    let list = bob
        .get(&format!("/api/v1/households/{bob_home}/recipes"))
        .await
        .json;
    let slugs: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["slug"].as_str().unwrap())
        .collect();
    assert_eq!(slugs, vec!["gratin-dauphinois", "gratin"]);
}

#[sqlx::test]
async fn refuser_avec_un_mot(db: PgPool) {
    let app = app(db).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    alice
        .put(
            &format!("/api/v1/households/{home}/recipes/gratin"),
            gratin("Gratin dauphinois"),
        )
        .await;
    let id = submit(&mut alice, home).await;
    let mut admin = admin(&app).await;
    let refused = admin
        .post(
            &format!("/api/v1/community/submissions/{id}/refuse"),
            json!({"message": "Il y en a déjà un très proche."}),
        )
        .await;
    assert_eq!(refused.status, StatusCode::NO_CONTENT, "{}", refused.text);
    let recipe = alice
        .get(&format!("/api/v1/households/{home}/recipes/gratin"))
        .await
        .json;
    assert_eq!(recipe["community"], false);
    assert_eq!(recipe["submission"]["status"], "refused");
    assert_eq!(
        recipe["submission"]["review_message"],
        "Il y en a déjà un très proche."
    );
    let notes = alice.get("/api/v1/me/notifications").await.json;
    assert_eq!(notes[0]["kind"], "submission_refused");
    assert!(
        notes[0]["text"]
            .as_str()
            .unwrap()
            .contains("Il y en a déjà un très proche.")
    );
    // Elle peut la reproposer.
    submit(&mut alice, home).await;
}

#[sqlx::test]
async fn copier_garde_les_cles_et_le_lien(db: PgPool) {
    let app = app(db).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    alice
        .put(
            &format!("/api/v1/households/{home}/recipes/gratin"),
            gratin("Gratin dauphinois"),
        )
        .await;
    let id = submit(&mut alice, home).await;
    let mut admin = admin(&app).await;
    admin
        .post(
            &format!("/api/v1/community/submissions/{id}/accept"),
            json!({}),
        )
        .await;

    let (mut bob, bob_home) = home_of(&app, "bob@example.org", "Chez Bob").await;
    let original = bob
        .get(&format!("/api/v1/households/{bob_home}/recipes/gratin"))
        .await
        .json;
    let copy = bob
        .post(
            &format!("/api/v1/households/{bob_home}/recipes/gratin/copy"),
            json!({}),
        )
        .await;
    assert_eq!(copy.status, StatusCode::CREATED, "{}", copy.text);
    assert_eq!(copy.json["slug"], "gratin-maison");
    assert_eq!(copy.json["community"], false);
    assert_eq!(copy.json["copied_from"]["slug"], "gratin");
    assert_eq!(copy.json["copied_from"]["version"], original["version"]);
    assert_eq!(copy.json["copied_from"]["behind"], false);
    assert_eq!(
        copy.json["variants"][0]["phases"][1]["key"],
        original["variants"][0]["phases"][1]["key"]
    );
    // Une copie ne se propose pas comme une nouvelle recette.
    let submit = bob
        .post(
            &format!("/api/v1/households/{bob_home}/recipes/gratin-maison/submit"),
            json!({}),
        )
        .await;
    assert_eq!(submit.status, StatusCode::UNPROCESSABLE_ENTITY);
    // On ne copie qu'une recette de la communauté.
    let own = bob
        .post(
            &format!("/api/v1/households/{bob_home}/recipes/gratin-maison/copy"),
            json!({"slug": "encore"}),
        )
        .await;
    assert_eq!(own.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test]
async fn reprendre_un_changement_de_l_originale(db: PgPool) {
    let app = app(db).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    alice
        .put(
            &format!("/api/v1/households/{home}/recipes/gratin"),
            gratin("Gratin dauphinois"),
        )
        .await;
    let id = submit(&mut alice, home).await;
    let mut admin = admin(&app).await;
    admin
        .post(
            &format!("/api/v1/community/submissions/{id}/accept"),
            json!({}),
        )
        .await;
    let (mut bob, bob_home) = home_of(&app, "bob@example.org", "Chez Bob").await;
    let copy_url = format!("/api/v1/households/{bob_home}/recipes/gratin-maison");
    bob.post(
        &format!("/api/v1/households/{bob_home}/recipes/gratin/copy"),
        json!({}),
    )
    .await;
    // Bob change le titre de sa copie.
    let mut mine = bob.get(&copy_url).await.json;
    let mut input = gratin("Le gratin de Bob");
    for (i, phase) in mine["variants"][0]["phases"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        input["variants"][0]["phases"][i]["key"] = phase["key"].take();
    }
    input["variants"][0]["key"] = mine["variants"][0]["key"].clone();
    input["ingredients"][0]["key"] = mine["ingredients"][0]["key"].clone();
    bob.put(&copy_url, input).await;

    // L'admin améliore la cuisson et le tranchage de l'originale.
    let mut better = admin_view(&mut admin, &app).await;
    better["variants"][0]["phases"][0]["text"] = json!("Trancher à 2 mm.");
    better["variants"][0]["phases"][1]["timer_s"] = json!(4200);
    let response = admin.put("/api/v1/community/recipes/gratin", better).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);

    let copy = bob.get(&copy_url).await.json;
    assert_eq!(copy["copied_from"]["behind"], true);
    let cuire = copy["variants"][0]["phases"][1]["key"].clone();
    let taken = bob
        .post(
            &format!("{copy_url}/upstream/take"),
            json!({"keys": [cuire], "seen": false}),
        )
        .await;
    assert_eq!(taken.status, StatusCode::OK, "{}", taken.text);
    assert_eq!(taken.json["variants"][0]["phases"][1]["timer_s"], 4200);
    // Ce qu'il n'a pas pris reste le sien ; le titre aussi.
    assert_eq!(
        taken.json["variants"][0]["phases"][0]["text"],
        "Trancher à 3 mm."
    );
    assert_eq!(taken.json["title"], "Le gratin de Bob");
    assert_eq!(taken.json["copied_from"]["behind"], true);
    // « Vu » : la copie suit la version courante de l'originale.
    let seen = bob
        .post(
            &format!("{copy_url}/upstream/take"),
            json!({"keys": [], "seen": true}),
        )
        .await;
    assert_eq!(seen.json["copied_from"]["behind"], false);
}

/// La recette de la communauté telle que l'admin la republie (clés comprises).
async fn admin_view(admin: &mut Browser, _app: &TestApp) -> Value {
    let home = admin.household("Chez l'admin").await;
    let mut recipe = admin
        .get(&format!("/api/v1/households/{home}/recipes/gratin"))
        .await
        .json;
    let object = recipe.as_object_mut().unwrap();
    for field in [
        "community",
        "retired",
        "version",
        "updated_at",
        "notes",
        "ratings",
        "copied_from",
        "submission",
    ] {
        object.remove(field);
    }
    for line in object["ingredients"].as_array_mut().unwrap() {
        let line = line.as_object_mut().unwrap();
        line.remove("ingredient_id");
        line.remove("name");
    }
    for variant in object["variants"].as_array_mut().unwrap() {
        let variant = variant.as_object_mut().unwrap();
        variant.remove("id");
        variant.remove("missing_equipment");
    }
    recipe
}
