//! Recettes de la communauté (visibles par tous les foyers, modifiées par l'admin de l'instance), slugs croisés,
//! clés stables des étapes, historique des versions.

mod common;

use axum::http::StatusCode;
use chaudron_server::{config::SignupPolicy, recipes};
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

/// Gratin dauphinois, une variante four en deux phases, la première avec deux gestes.
fn gratin() -> Value {
    json!({
        "title": "Gratin dauphinois",
        "ingredients": [
            {"ingredient": "potato", "qty": 250, "unit": "g", "role": "starch"},
            {"ingredient": "cream", "qty": 50, "unit": "ml", "role": "fat"},
            {"ingredient": "garlic", "qty": 1, "unit": "piece", "role": "other", "per_batch": true},
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

/// La recette passe à la communauté (le chemin normal, proposer puis accepter, vient plus tard).
async fn to_community(db: &PgPool, slug: &str) {
    sqlx::query("UPDATE recipes SET household_id = NULL WHERE slug = $1")
        .bind(slug)
        .execute(db)
        .await
        .unwrap();
}

#[sqlx::test]
async fn le_foyer_voit_la_communaute_dans_menus_courses_et_cuisson(db: PgPool) {
    let app = app(db.clone()).await;
    let (mut alice, alice_home) = home_of(&app, "alice@example.org", "Maison").await;
    let url = format!("/api/v1/households/{alice_home}/recipes/gratin-dauphinois");
    assert_eq!(alice.put(&url, gratin()).await.status, StatusCode::CREATED);
    to_community(&db, "gratin-dauphinois").await;

    // Bob, d'un autre foyer, la voit, la prévoit, fait ses courses et la cuisine.
    let (mut bob, home) = home_of(&app, "bob@example.org", "Chez Bob").await;
    let list = bob
        .get(&format!("/api/v1/households/{home}/recipes"))
        .await
        .json;
    assert_eq!(list[0]["slug"], "gratin-dauphinois");
    assert_eq!(list[0]["community"], true);
    let mine = bob
        .get(&format!(
            "/api/v1/households/{home}/recipes?scope=household"
        ))
        .await
        .json;
    assert_eq!(mine, json!([]));
    let recipe = bob
        .get(&format!(
            "/api/v1/households/{home}/recipes/gratin-dauphinois"
        ))
        .await;
    assert_eq!(recipe.status, StatusCode::OK, "{}", recipe.text);
    assert_eq!(recipe.json["community"], true);
    assert_eq!(recipe.json["retired"], false);

    let menu = bob
        .put(
            &format!("/api/v1/households/{home}/menus/2026-11-02"),
            json!({"slots": [{"date": "2026-11-02", "meal": "dinner", "recipe": "gratin-dauphinois"}]}),
        )
        .await;
    assert!(menu.status.is_success(), "{}", menu.text);
    let shopping = bob
        .get(&format!(
            "/api/v1/households/{home}/menus/2026-11-02/shopping"
        ))
        .await;
    assert_eq!(shopping.status, StatusCode::OK, "{}", shopping.text);
    assert!(
        shopping.text.contains("Pomme de terre"),
        "{}",
        shopping.text
    );
    let agenda = bob
        .get(&format!(
            "/api/v1/households/{home}/agenda?from=2026-11-02&days=1"
        ))
        .await;
    assert_eq!(agenda.json["recipes"][0]["slug"], "gratin-dauphinois");
    let cooking = bob
        .put(
            &format!("/api/v1/households/{home}/cooking"),
            json!({"recipe": "gratin-dauphinois", "date": "2026-11-02", "meal": "dinner", "mode": "fast", "phase": 1}),
        )
        .await;
    assert_eq!(cooking.status, StatusCode::OK, "{}", cooking.text);
    assert_eq!(cooking.json["current"]["title"], "Cuire");

    // Ni modifiée ni supprimée par un foyer.
    let put = bob
        .put(
            &format!("/api/v1/households/{home}/recipes/gratin-dauphinois"),
            gratin(),
        )
        .await;
    assert_eq!(put.status, StatusCode::CONFLICT);
    assert_eq!(put.json["error"], "slug_taken");
    let delete = bob
        .delete(&format!(
            "/api/v1/households/{home}/recipes/gratin-dauphinois"
        ))
        .await;
    assert_eq!(delete.status, StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn slug_de_la_communaute_refuse_au_foyer(db: PgPool) {
    let app = app(db.clone()).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    let url = format!("/api/v1/households/{home}/recipes/gratin-dauphinois");
    alice.put(&url, gratin()).await;
    to_community(&db, "gratin-dauphinois").await;

    let (mut bob, bob_home) = home_of(&app, "bob@example.org", "Chez Bob").await;
    let response = bob
        .put(
            &format!("/api/v1/households/{bob_home}/recipes/gratin-dauphinois"),
            gratin(),
        )
        .await;
    assert_eq!(response.status, StatusCode::CONFLICT);
    assert!(
        response.json["message"]
            .as_str()
            .unwrap()
            .contains("communauté")
    );
    // Un autre slug passe.
    let response = bob
        .put(
            &format!("/api/v1/households/{bob_home}/recipes/gratin-maison"),
            gratin(),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED);
}

#[sqlx::test]
async fn l_import_ignore_un_slug_de_la_communaute_et_le_dit(db: PgPool) {
    let app = app(db.clone()).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    alice
        .put(
            &format!("/api/v1/households/{home}/recipes/gratin-dauphinois"),
            gratin(),
        )
        .await;
    to_community(&db, "gratin-dauphinois").await;
    let (mut bob, bob_home) = home_of(&app, "bob@example.org", "Chez Bob").await;

    let mut recipe = gratin();
    recipe["slug"] = json!("gratin-dauphinois");
    recipe["title"] = json!("Mon gratin");
    let response = bob
        .post(
            &format!("/api/v1/households/{bob_home}/import"),
            json!({
                "recipes": [recipe],
                "menu": {"week_start": "2026-11-02",
                         "slots": [{"date": "2026-11-02", "meal": "dinner", "recipe": "gratin-dauphinois"}]},
            }),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    assert_eq!(response.json["recipes_created"], json!([]));
    assert_eq!(
        response.json["recipe_warnings"][0]["slug"],
        "gratin-dauphinois"
    );
    assert_eq!(response.json["recipe_warnings"][0]["code"], "slug_taken");
    // Le menu prend celle de la communauté.
    let menu = bob
        .get(&format!("/api/v1/households/{bob_home}/menus/2026-11-02"))
        .await
        .json;
    assert_eq!(menu["slots"][0]["recipe"]["title"], "Gratin dauphinois");
}

#[sqlx::test]
async fn chaque_enregistrement_cree_une_version(db: PgPool) {
    let app = app(db).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    let url = format!("/api/v1/households/{home}/recipes/gratin-dauphinois");
    let first = alice.put(&url, gratin()).await;
    assert_eq!(first.json["version"], 1);

    let mut changed = gratin();
    changed["variants"][0]["phases"][1]["text"] = json!("Four à 150 °C, 1 h 15.");
    changed["message"] = json!("moins chaud, plus longtemps");
    let second = alice.put(&url, changed.clone()).await;
    assert_eq!(second.json["version"], 2);
    // Le même contenu republié ne fait pas de version.
    let same = alice.put(&url, changed).await;
    assert_eq!(same.json["version"], 2);

    // Claude publie avec son jeton : la version est à son nom.
    let token = alice
        .post(
            &format!("/api/v1/households/{home}/tokens"),
            json!({"name": "Claude", "scopes": ["recipes:write", "read"]}),
        )
        .await
        .json["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut claude = app.api_client(&token);
    let mut by_claude = gratin();
    by_claude["title"] = json!("Gratin dauphinois du dimanche");
    assert_eq!(claude.put(&url, by_claude).await.json["version"], 3);

    let versions = alice.get(&format!("{url}/versions")).await;
    assert_eq!(versions.status, StatusCode::OK, "{}", versions.text);
    let versions = versions.json;
    assert_eq!(versions.as_array().unwrap().len(), 3);
    assert_eq!(versions[0]["number"], 3);
    assert_eq!(versions[0]["author_name"], "Claude");
    assert_eq!(versions[1]["message"], "moins chaud, plus longtemps");
    assert_eq!(versions[2]["author_name"], "alice");

    let v1 = alice.get(&format!("{url}/versions/1")).await;
    assert_eq!(v1.status, StatusCode::OK, "{}", v1.text);
    assert_eq!(
        v1.json["content"]["variants"][0]["phases"][1]["text"],
        "Four à 160 °C, 1 h."
    );
}

#[sqlx::test]
async fn revenir_a_une_version_en_cree_une_nouvelle(db: PgPool) {
    let app = app(db).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    let url = format!("/api/v1/households/{home}/recipes/gratin-dauphinois");
    alice.put(&url, gratin()).await;
    let mut changed = gratin();
    changed["title"] = json!("Gratin raté");
    alice.put(&url, changed).await;

    let restored = alice
        .post(&format!("{url}/versions/1/restore"), json!({}))
        .await;
    assert_eq!(restored.status, StatusCode::OK, "{}", restored.text);
    assert_eq!(restored.json["title"], "Gratin dauphinois");
    assert_eq!(restored.json["version"], 3);
    let versions = alice.get(&format!("{url}/versions")).await.json;
    assert_eq!(versions[0]["message"], "Retour à la version 1");
    assert_eq!(
        alice
            .post(&format!("{url}/versions/9/restore"), json!({}))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

fn keys(recipe: &Value) -> Vec<String> {
    let mut keys = Vec::new();
    for line in recipe["ingredients"].as_array().unwrap() {
        keys.push(line["key"].as_str().unwrap().to_owned());
    }
    for variant in recipe["variants"].as_array().unwrap() {
        keys.push(variant["key"].as_str().unwrap().to_owned());
        for phase in variant["phases"].as_array().unwrap() {
            keys.push(phase["key"].as_str().unwrap().to_owned());
            for step in phase["steps"].as_array().unwrap() {
                keys.push(step["key"].as_str().unwrap().to_owned());
            }
        }
    }
    keys
}

#[sqlx::test]
async fn les_cles_survivent_a_l_editeur_et_a_l_import(db: PgPool) {
    let app = app(db).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    let url = format!("/api/v1/households/{home}/recipes/gratin-dauphinois");
    let first = alice.put(&url, gratin()).await.json;
    let before = keys(&first);
    assert_eq!(before.len(), 3 + 1 + 2 + 2);

    // Republiée sans clés (l'import de Claude) : chaque élément garde la sienne, par sa place.
    let mut tweaked = gratin();
    tweaked["variants"][0]["phases"][1]["timer_s"] = json!(4200);
    let again = alice.put(&url, tweaked).await.json;
    assert_eq!(keys(&again), before);

    // L'éditeur renvoie les clés : une phase insérée en tête n'en vole aucune.
    let mut edited = gratin();
    let phases = &first["variants"][0]["phases"];
    edited["variants"][0]["key"] = first["variants"][0]["key"].clone();
    edited["variants"][0]["phases"] = json!([
        {"title": "Préchauffer", "text": "Four à 160 °C."},
        {"key": phases[0]["key"], "title": "Les pommes de terre", "text": "Éplucher, trancher à 3 mm.",
         "steps": [{"key": phases[0]["steps"][0]["key"], "text": "Éplucher."},
                   {"key": phases[0]["steps"][1]["key"], "text": "Trancher à 3 mm."}]},
        {"key": phases[1]["key"], "title": "Cuire", "text": "Four à 160 °C, 1 h.", "timer_s": 3600},
    ]);
    let after = alice.put(&url, edited).await.json;
    let new_phases = &after["variants"][0]["phases"];
    assert_eq!(new_phases[1]["key"], phases[0]["key"]);
    assert_eq!(new_phases[2]["key"], phases[1]["key"]);
    assert_eq!(
        new_phases[1]["steps"][1]["key"],
        phases[0]["steps"][1]["key"]
    );
    assert!(!before.contains(&new_phases[0]["key"].as_str().unwrap().to_owned()));
}

#[sqlx::test]
async fn seul_l_admin_modifie_la_communaute(db: PgPool) {
    let app = app(db.clone()).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    alice
        .put(
            &format!("/api/v1/households/{home}/recipes/gratin-dauphinois"),
            gratin(),
        )
        .await;
    to_community(&db, "gratin-dauphinois").await;
    assert_eq!(alice.me().await["instance_admin"], false);

    let community = "/api/v1/community/recipes/gratin-dauphinois";
    let mut changed = gratin();
    changed["title"] = json!("Gratin dauphinois de la communauté");
    assert_eq!(
        alice.put(community, changed.clone()).await.status,
        StatusCode::FORBIDDEN
    );

    let mut admin = app.browser();
    admin.login(ADMIN).await;
    assert_eq!(admin.me().await["instance_admin"], true);
    let response = admin.put(community, changed).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    assert_eq!(response.json["version"], 2);
    assert_eq!(
        admin
            .put("/api/v1/community/recipes/inconnue", gratin())
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    // Le foyer ne restaure pas une version de la communauté ; l'admin, oui.
    let restore = format!("/api/v1/households/{home}/recipes/gratin-dauphinois/versions/1/restore");
    assert_eq!(
        alice.post(&restore, json!({})).await.status,
        StatusCode::FORBIDDEN
    );
    let admin_home = admin.household("Admin").await;
    let versions = format!("/api/v1/households/{admin_home}/recipes/gratin-dauphinois/versions");
    // La version 1 date du foyer d'Alice : elle reste au foyer, même pour l'admin.
    assert_eq!(
        admin
            .post(&format!("{versions}/1/restore"), json!({}))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let mut back = gratin();
    back["title"] = json!("Gratin dauphinois, revu");
    admin.put(community, back).await;
    assert_eq!(
        admin
            .post(&format!("{versions}/2/restore"), json!({}))
            .await
            .status,
        StatusCode::OK
    );
}

#[sqlx::test]
async fn admin_par_adresse_verifiee_seulement(db: PgPool) {
    let app = app(db.clone()).await;
    let mut admin = app.browser();
    admin.login(ADMIN).await;
    assert_eq!(admin.me().await["instance_admin"], true);
    // La même adresse, non vérifiée par le fournisseur (Amazon) : pas admin.
    sqlx::query("UPDATE identities SET email_verified = false")
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(admin.me().await["instance_admin"], false);
}

#[sqlx::test]
async fn retirer_une_recette_de_la_communaute(db: PgPool) {
    let app = app(db.clone()).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    for slug in ["gratin-dauphinois", "gratin-savoyard"] {
        alice
            .put(
                &format!("/api/v1/households/{home}/recipes/{slug}"),
                gratin(),
            )
            .await;
        to_community(&db, slug).await;
    }
    alice
        .put(
            &format!("/api/v1/households/{home}/menus/2026-11-02"),
            json!({"slots": [{"date": "2026-11-02", "meal": "dinner", "recipe": "gratin-dauphinois"}]}),
        )
        .await;
    let mut admin = app.browser();
    admin.login(ADMIN).await;

    // Utilisée par un menu : retirée du catalogue, gardée par le menu.
    let response = admin
        .delete("/api/v1/community/recipes/gratin-dauphinois")
        .await;
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", response.text);
    let list = alice
        .get(&format!("/api/v1/households/{home}/recipes"))
        .await
        .json;
    assert_eq!(list.as_array().unwrap().len(), 1);
    let recipe = alice
        .get(&format!(
            "/api/v1/households/{home}/recipes/gratin-dauphinois"
        ))
        .await
        .json;
    assert_eq!(recipe["retired"], true);
    let menu = alice
        .get(&format!("/api/v1/households/{home}/menus/2026-11-02"))
        .await;
    assert_eq!(menu.status, StatusCode::OK);
    // Pas utilisée : supprimée.
    admin
        .delete("/api/v1/community/recipes/gratin-savoyard")
        .await;
    assert_eq!(
        alice
            .get(&format!(
                "/api/v1/households/{home}/recipes/gratin-savoyard"
            ))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
async fn version_de_depart_des_recettes_existantes(db: PgPool) {
    let app = app(db.clone()).await;
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    let url = format!("/api/v1/households/{home}/recipes/gratin-dauphinois");
    alice.put(&url, gratin()).await;
    // Une recette d'avant l'historique.
    sqlx::query("DELETE FROM recipe_versions")
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("UPDATE recipes SET version = 0")
        .execute(&db)
        .await
        .unwrap();

    recipes::backfill_versions(&db).await.unwrap();
    recipes::backfill_versions(&db).await.unwrap();
    let versions = alice.get(&format!("{url}/versions")).await.json;
    assert_eq!(versions.as_array().unwrap().len(), 1);
    assert_eq!(versions[0]["number"], 1);
    assert_eq!(alice.get(&url).await.json["version"], 1);
}
