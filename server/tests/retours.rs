//! Retours de la communauté : commentaires, propositions de changement par étape, variantes tentées, fils et « +1 ».
//! Un retour n'est visible que du foyer de son auteur tant que l'admin ne l'a pas publié ; l'admin publie,
//! applique (une nouvelle version), ferme ou refuse, et l'auteur est prévenu.

mod common;

use axum::http::StatusCode;
use chaudron_server::config::SignupPolicy;
use common::{Browser, TestApp, config, link_alexa};
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

fn hachis() -> Value {
    json!({
        "title": "Hachis parmentier",
        "ingredients": [
            {"ingredient": "minced_beef", "qty": 150, "unit": "g", "role": "protein"},
            {"ingredient": "potato", "qty": 250, "unit": "g", "role": "starch"},
        ],
        "variants": [{
            "label": "Cookeo",
            "active_min": 20,
            "total_min": 60,
            "phases": [
                {"title": "Les légumes", "text": "Cubes de 3 cm, vapeur 12 min.", "timer_s": 720},
                {"title": "La farce", "text": "Revenir le bœuf.",
                 "steps": [{"text": "Feu vif."}, {"text": "Écraser à la spatule."}]},
            ],
        }],
    })
}

/// Le hachis, dans la communauté (proposé par l'admin et accepté), et trois foyers qui le voient.
struct World {
    app: TestApp,
    admin: Browser,
    admin_home: Uuid,
    alice: Browser,
    home: Uuid,
    bob: Browser,
    bob_home: Uuid,
    recipe: Value,
}

async fn world(db: PgPool) -> World {
    let app = app(db).await;
    let (mut admin, admin_home) = home_of(&app, ADMIN, "Chez l'admin").await;
    admin
        .put(
            &format!("/api/v1/households/{admin_home}/recipes/hachis"),
            hachis(),
        )
        .await;
    let submitted = admin
        .post(
            &format!("/api/v1/households/{admin_home}/recipes/hachis/submit"),
            json!({}),
        )
        .await;
    let id = submitted.json["id"].as_str().unwrap().to_owned();
    let accepted = admin
        .post(
            &format!("/api/v1/community/submissions/{id}/accept"),
            json!({}),
        )
        .await;
    assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.text);
    let (mut alice, home) = home_of(&app, "alice@example.org", "Maison").await;
    let (bob, bob_home) = home_of(&app, "bob@example.org", "Chez Bob").await;
    let recipe = alice
        .get(&format!("/api/v1/households/{home}/recipes/hachis"))
        .await
        .json;
    World {
        app,
        admin,
        admin_home,
        alice,
        home,
        bob,
        bob_home,
        recipe,
    }
}

fn feedback_url(home: Uuid) -> String {
    format!("/api/v1/households/{home}/recipes/hachis/feedback")
}

#[sqlx::test]
async fn retour_en_attente_invisible_des_autres_foyers(db: PgPool) {
    let mut w = world(db).await;
    let vapeur = w.recipe["variants"][0]["phases"][0]["key"].clone();
    let proposal = w
        .alice
        .post(
            &feedback_url(w.home),
            json!({"visibility": "community", "kind": "proposal", "element_key": vapeur,
                   "body": "Le cœur restait ferme.", "changes": {"timer_s": 840}}),
        )
        .await;
    assert_eq!(proposal.status, StatusCode::CREATED, "{}", proposal.text);
    assert_eq!(proposal.json["status"], "pending");
    let comment = w
        .alice
        .post(
            &feedback_url(w.home),
            json!({"visibility": "community", "kind": "comment", "element_key": vapeur, "body": "Ça a débordé."}),
        )
        .await;
    assert_eq!(comment.status, StatusCode::CREATED, "{}", comment.text);

    // Le foyer d'Alice voit ses retours en attente (sur la fiche, dans la cuisson) ; Bob, rien, par aucun chemin.
    let mine = w.alice.get(&feedback_url(w.home)).await.json;
    assert_eq!(mine.as_array().unwrap().len(), 2);
    assert_eq!(mine[0]["mine"], true);
    let theirs = w
        .bob
        .get(&format!("{}?closed=true", feedback_url(w.bob_home)))
        .await
        .json;
    assert_eq!(theirs, json!([]));
    let recipe = w
        .bob
        .get(&format!("/api/v1/households/{}/recipes/hachis", w.bob_home))
        .await
        .json;
    assert_eq!(recipe["notes"], json!([]));
    w.bob
        .put(
            &format!("/api/v1/households/{}/cooking", w.bob_home),
            json!({"recipe": "hachis", "mode": "fast", "phase": 0}),
        )
        .await;
    let token = link_alexa(&w.app, &mut w.bob, w.bob_home).await;
    let mut alexa = w.app.api_client(&token);
    let cooking = alexa
        .get(&format!("/api/v1/households/{}/cooking", w.bob_home))
        .await
        .json;
    assert_eq!(cooking["step_notes"], json!([]));
    let agenda = w
        .bob
        .get(&format!(
            "/api/v1/households/{}/recipes/hachis/feedback",
            w.bob_home
        ))
        .await
        .json;
    assert_eq!(agenda, json!([]));

    // Publié : tout le monde le voit, Alexa le lit (un commentaire, pas une proposition).
    let id = comment.json["id"].as_str().unwrap();
    let published = w
        .admin
        .post(
            &format!("/api/v1/community/feedback/{id}/publish"),
            json!({}),
        )
        .await;
    assert_eq!(
        published.status,
        StatusCode::NO_CONTENT,
        "{}",
        published.text
    );
    let cooking = alexa
        .get(&format!("/api/v1/households/{}/cooking", w.bob_home))
        .await
        .json;
    assert_eq!(cooking["step_notes"][0]["body"], "Ça a débordé.");
    let theirs = w.bob.get(&feedback_url(w.bob_home)).await.json;
    assert_eq!(theirs.as_array().unwrap().len(), 1);
    assert_eq!(theirs[0]["mine"], false);
    assert_eq!(theirs[0]["household_name"], "Maison");
}

#[sqlx::test]
async fn appliquer_une_proposition_cree_une_version(db: PgPool) {
    let mut w = world(db).await;
    let vapeur = w.recipe["variants"][0]["phases"][0]["key"].clone();
    let proposal = w
        .alice
        .post(
            &feedback_url(w.home),
            json!({"visibility": "community", "kind": "proposal", "element_key": vapeur,
                   "body": "Le cœur restait ferme.",
                   "changes": {"timer_s": 840, "text": "Cubes de 2 cm, vapeur 14 min."}}),
        )
        .await;
    let id = proposal.json["id"].as_str().unwrap().to_owned();

    // « À revoir » : la recette et ses retours ouverts.
    assert_eq!(
        w.alice.get("/api/v1/community/review").await.status,
        StatusCode::FORBIDDEN
    );
    let review = w.admin.get("/api/v1/community/review").await.json;
    assert_eq!(review["recipes"][0]["slug"], "hachis");
    assert_eq!(review["recipes"][0]["feedback"][0]["id"], id.as_str());
    assert_eq!(review["recipes"][0]["feedback"][0]["stale"], false);

    let applied = w
        .admin
        .post(&format!("/api/v1/community/feedback/{id}/apply"), json!({}))
        .await;
    assert_eq!(applied.status, StatusCode::OK, "{}", applied.text);
    assert_eq!(applied.json["variants"][0]["phases"][0]["timer_s"], 840);
    assert_eq!(
        applied.json["variants"][0]["phases"][0]["text"],
        "Cubes de 2 cm, vapeur 14 min."
    );
    let versions = w
        .alice
        .get(&format!(
            "/api/v1/households/{}/recipes/hachis/versions",
            w.home
        ))
        .await
        .json;
    assert_eq!(
        versions[0]["message"],
        "Proposition du foyer Maison intégrée"
    );
    // La discussion est fermée ; Alice est prévenue.
    assert_eq!(w.alice.get(&feedback_url(w.home)).await.json, json!([]));
    let closed = w
        .alice
        .get(&format!("{}?closed=true", feedback_url(w.home)))
        .await
        .json;
    assert_eq!(closed[0]["status"], "applied");
    let notes = w.alice.get("/api/v1/me/notifications").await.json;
    assert_eq!(notes[0]["kind"], "feedback_applied");
    assert_eq!(
        notes[0]["text"],
        "« Hachis parmentier » a été amélioré avec ton retour."
    );
    assert_eq!(
        w.admin
            .post(&format!("/api/v1/community/feedback/{id}/apply"), json!({}))
            .await
            .status,
        StatusCode::CONFLICT
    );
}

#[sqlx::test]
async fn etape_changee_depuis_la_proposition(db: PgPool) {
    let mut w = world(db).await;
    let vapeur = w.recipe["variants"][0]["phases"][0]["key"].clone();
    let proposal = w
        .alice
        .post(
            &feedback_url(w.home),
            json!({"visibility": "community", "kind": "proposal", "element_key": vapeur,
                   "body": "Plus long.", "changes": {"timer_s": 840}}),
        )
        .await;
    let id = proposal.json["id"].as_str().unwrap().to_owned();
    // L'admin change le minuteur entre-temps.
    let mut changed = w
        .admin
        .get(&format!(
            "/api/v1/households/{}/recipes/hachis",
            w.admin_home
        ))
        .await
        .json;
    changed["variants"][0]["phases"][0]["timer_s"] = json!(780);
    let input = json!({
        "title": changed["title"], "ingredients": hachis()["ingredients"],
        "variants": [{"label": "Cookeo", "active_min": 20, "total_min": 60, "phases": [
            {"key": vapeur, "title": "Les légumes", "text": "Cubes de 3 cm, vapeur 12 min.", "timer_s": 780},
            {"title": "La farce", "text": "Revenir le bœuf.",
             "steps": [{"text": "Feu vif."}, {"text": "Écraser à la spatule."}]},
        ]}],
    });
    assert_eq!(
        w.admin
            .put("/api/v1/community/recipes/hachis", input)
            .await
            .status,
        StatusCode::OK
    );

    let review = w.admin.get("/api/v1/community/review").await.json;
    assert_eq!(review["recipes"][0]["feedback"][0]["stale"], true);
    let blind = w
        .admin
        .post(&format!("/api/v1/community/feedback/{id}/apply"), json!({}))
        .await;
    assert_eq!(blind.status, StatusCode::CONFLICT);
    assert_eq!(blind.json["error"], "step_changed");

    // « Modifier puis appliquer » : l'admin publie la recette telle qu'il l'a revue.
    let mut edited = w
        .admin
        .get(&format!(
            "/api/v1/households/{}/recipes/hachis",
            w.admin_home
        ))
        .await
        .json;
    edited["variants"][0]["phases"][0]["timer_s"] = json!(840);
    let recipe = json!({
        "title": "Hachis parmentier", "ingredients": hachis()["ingredients"],
        "variants": [{"label": "Cookeo", "active_min": 20, "total_min": 60, "phases": [
            {"key": vapeur, "title": "Les légumes", "text": "Cubes de 3 cm, vapeur 14 min.", "timer_s": 840},
            {"title": "La farce", "text": "Revenir le bœuf.",
             "steps": [{"text": "Feu vif."}, {"text": "Écraser à la spatule."}]},
        ]}],
    });
    let applied = w
        .admin
        .post(
            &format!("/api/v1/community/feedback/{id}/apply"),
            json!({"recipe": recipe}),
        )
        .await;
    assert_eq!(applied.status, StatusCode::OK, "{}", applied.text);
    assert_eq!(applied.json["variants"][0]["phases"][0]["timer_s"], 840);
}

#[sqlx::test]
async fn fils_appuis_refus_et_note_gardee(db: PgPool) {
    let mut w = world(db).await;
    let comment = w
        .alice
        .post(
            &feedback_url(w.home),
            json!({"visibility": "community", "kind": "comment", "body": "Trop de purée pour 4."}),
        )
        .await;
    let id = comment.json["id"].as_str().unwrap().to_owned();
    w.admin
        .post(
            &format!("/api/v1/community/feedback/{id}/publish"),
            json!({}),
        )
        .await;

    // Bob appuie et répond : sa réponse attend l'admin ; le foyer d'Alice ne la voit pas encore.
    let support = w
        .bob
        .post(
            &format!("/api/v1/households/{}/feedback/{id}/support", w.bob_home),
            json!({}),
        )
        .await;
    assert_eq!(support.status, StatusCode::NO_CONTENT, "{}", support.text);
    let reply = w
        .bob
        .post(
            &format!("/api/v1/households/{}/feedback/{id}/replies", w.bob_home),
            json!({"body": "Pareil chez nous."}),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.text);
    assert_eq!(reply.json["status"], "pending");
    let seen = w.alice.get(&feedback_url(w.home)).await.json;
    assert_eq!(seen.as_array().unwrap().len(), 1);
    assert_eq!(seen[0]["supports"], 1);
    assert_eq!(seen[0]["supported"], false);
    let bob_sees = w.bob.get(&feedback_url(w.bob_home)).await.json;
    assert_eq!(bob_sees.as_array().unwrap().len(), 2);
    assert_eq!(bob_sees[1]["parent_id"], id.as_str());
    assert_eq!(bob_sees[0]["supported"], true);

    // Sous son propre retour encore en attente, la réponse du foyer attend aussi : Bob ne la voit pas.
    let waiting = w
        .alice
        .post(
            &feedback_url(w.home),
            json!({"visibility": "community", "kind": "comment", "body": "Pas encore relu."}),
        )
        .await;
    let waiting_id = waiting.json["id"].as_str().unwrap().to_owned();
    let under = w
        .alice
        .post(
            &format!(
                "/api/v1/households/{}/feedback/{waiting_id}/replies",
                w.home
            ),
            json!({"body": "Et sa réponse."}),
        )
        .await;
    assert_eq!(under.json["status"], "pending");
    let bob_sees = w.bob.get(&feedback_url(w.bob_home)).await.json;
    assert!(
        bob_sees
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["body"] != "Et sa réponse.")
    );
    w.alice
        .delete(&format!(
            "/api/v1/households/{}/feedback/{waiting_id}",
            w.home
        ))
        .await;

    // Dans un fil publié, une réponse de son propre foyer est publiée d'emblée.
    let own = w
        .alice
        .post(
            &format!("/api/v1/households/{}/feedback/{id}/replies", w.home),
            json!({"body": "Merci !"}),
        )
        .await;
    assert_eq!(own.json["status"], "published");

    // Refusé, avec un mot ; Alice le garde comme note du cahier.
    let other = w
        .alice
        .post(&feedback_url(w.home), json!({"visibility": "community", "kind": "comment", "body": "Remplacer le bœuf par du soja."}))
        .await;
    let other_id = other.json["id"].as_str().unwrap().to_owned();
    let refused = w
        .admin
        .post(
            &format!("/api/v1/community/feedback/{other_id}/refuse"),
            json!({"message": "On garde le bœuf ici."}),
        )
        .await;
    assert_eq!(refused.status, StatusCode::NO_CONTENT);
    let notes = w.alice.get("/api/v1/me/notifications").await.json;
    assert_eq!(notes[0]["kind"], "feedback_refused");
    let kept = w
        .alice
        .patch(
            &format!("/api/v1/households/{}/feedback/{other_id}", w.home),
            json!({"visibility": "household"}),
        )
        .await;
    assert_eq!(kept.status, StatusCode::OK, "{}", kept.text);
    assert_eq!(kept.json["visibility"], "household");
    assert_eq!(kept.json["status"], "open");

    // Une note du cahier part à la communauté.
    let note = w
        .alice
        .post(
            &feedback_url(w.home),
            json!({"body": "Gratiner 5 min de plus."}),
        )
        .await;
    let note_id = note.json["id"].as_str().unwrap();
    let shared = w
        .alice
        .patch(
            &format!("/api/v1/households/{}/feedback/{note_id}", w.home),
            json!({"visibility": "community"}),
        )
        .await;
    assert_eq!(shared.json["visibility"], "community");
    assert_eq!(shared.json["status"], "pending");
}

#[sqlx::test]
async fn proposer_les_changements_d_une_copie(db: PgPool) {
    let mut w = world(db).await;
    let copy = w
        .bob
        .post(
            &format!("/api/v1/households/{}/recipes/hachis/copy", w.bob_home),
            json!({}),
        )
        .await
        .json;
    let url = format!("/api/v1/households/{}/recipes/hachis-maison", w.bob_home);
    let mut input = json!({
        "title": "Hachis de Bob",
        "ingredients": [
            {"key": copy["ingredients"][0]["key"], "ingredient": "minced_beef", "qty": 120, "unit": "g", "role": "protein"},
            {"key": copy["ingredients"][1]["key"], "ingredient": "potato", "qty": 250, "unit": "g", "role": "starch"},
        ],
        "variants": [{"key": copy["variants"][0]["key"], "label": "Cookeo", "active_min": 20, "total_min": 60, "phases": [
            {"key": copy["variants"][0]["phases"][0]["key"], "title": "Les légumes",
             "text": "Cubes de 2 cm, vapeur 12 min.", "timer_s": 720},
            {"key": copy["variants"][0]["phases"][1]["key"], "title": "La farce", "text": "Revenir le bœuf.",
             "steps": [{"key": copy["variants"][0]["phases"][1]["steps"][0]["key"], "text": "Feu vif."},
                       {"key": copy["variants"][0]["phases"][1]["steps"][1]["key"], "text": "Écraser à la spatule."}]},
            {"title": "Gratiner", "text": "Four 220 °C, 15 min."},
        ]}],
    });
    input["message"] = json!("Notre façon");
    assert_eq!(w.bob.put(&url, input).await.status, StatusCode::OK);

    let proposed = w
        .bob
        .post(
            &format!("{url}/propose-changes"),
            json!({"message": "Ce qu'on fait chez nous."}),
        )
        .await;
    assert_eq!(proposed.status, StatusCode::CREATED, "{}", proposed.text);
    let kinds: Vec<&str> = proposed
        .json
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["kind"].as_str().unwrap())
        .collect();
    // Une proposition par étape ou ligne changée ; le reste (le titre, la phase ajoutée) en un commentaire.
    assert_eq!(kinds, vec!["proposal", "proposal", "comment"]);
    assert_eq!(
        proposed.json[0]["changes"]["qty"],
        json!({"from": 150.0, "to": 120.0})
    );
    assert_eq!(
        proposed.json[1]["changes"]["text"]["to"],
        "Cubes de 2 cm, vapeur 12 min."
    );
    assert!(
        proposed.json[2]["body"]
            .as_str()
            .unwrap()
            .contains("Gratiner")
    );
    let review = w.admin.get("/api/v1/community/review").await.json;
    assert_eq!(
        review["recipes"][0]["feedback"].as_array().unwrap().len(),
        3
    );
    assert_eq!(
        review["recipes"][0]["feedback"][0]["household_name"],
        "Chez Bob"
    );
}
