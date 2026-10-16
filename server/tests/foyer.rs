//! Foyer, membres, équipements, droits et invitations.

mod common;

use axum::http::StatusCode;
use chaudron_server::config::SignupPolicy;
use common::{Browser, TestApp};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// Alice, connectée, propriétaire du foyer « Maison ».
async fn alice_and_home(app: &TestApp) -> (Browser, Uuid) {
    let mut alice = app.browser();
    alice.login("alice@example.org").await;
    let home = alice.household("Maison").await;
    (alice, home)
}

/// Bob rejoint le foyer par une invitation d'Alice.
async fn bob_joins(app: &TestApp, alice: &mut Browser, home: Uuid) -> Browser {
    let token = alice.invite(home).await;
    let mut bob = app.browser();
    assert_eq!(
        bob.login_with("bob@example.org", Some(&token)).await,
        format!("/invitation/{token}")
    );
    bob
}

async fn ingredient_id(browser: &mut Browser, code: &str) -> String {
    let list = browser.get("/api/v1/ingredients").await.json;
    list.as_array()
        .unwrap()
        .iter()
        .find(|i| i["code"] == code)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[sqlx::test]
async fn creer_et_modifier_le_foyer(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let mut alice = app.browser();
    alice.login("alice@example.org").await;

    let response = alice
        .post("/api/v1/households", json!({"name": "  Maison  "}))
        .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let household = response.json;
    assert_eq!(household["name"], "Maison");
    assert_eq!(household["timezone"], "Europe/Paris");
    assert_eq!(household["role"], "owner");
    assert_eq!(household["users"][0]["display_name"], "alice");
    assert_eq!(household["users"][0]["role"], "owner");

    let me = alice.me().await;
    assert_eq!(me["households"][0]["name"], "Maison");
    assert_eq!(me["households"][0]["role"], "owner");

    let id = household["id"].as_str().unwrap();
    let response = alice
        .patch(
            &format!("/api/v1/households/{id}"),
            json!({"name": "Les B."}),
        )
        .await;
    assert_eq!(response.json["name"], "Les B.");
    assert_eq!(response.json["timezone"], "Europe/Paris");

    let response = alice.post("/api/v1/households", json!({"name": ""})).await;
    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.json["error"], "invalid");
}

#[sqlx::test]
async fn membre_avec_bareme_par_defaut(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = alice_and_home(&app).await;
    let members = format!("/api/v1/households/{home}/members");

    let response = alice
        .post(&members, json!({"name": "Léo", "kind": "teen"}))
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
    let leo = response.json;
    assert_eq!(leo["diet"], "omnivore");
    assert_eq!(leo["user_id"], Value::Null);
    assert_eq!(
        leo["portions"],
        json!({"protein": 1.0, "starch": 2.0, "veg": 1.0, "fat": 1.0, "other": 1.0})
    );
    assert_eq!(leo["presence"]["lunch"], json!(vec![true; 7]));
    assert_eq!(leo["presence"]["dinner"], json!(vec![true; 7]));
    assert_eq!(leo["exclusions"], json!([]));

    let response = alice
        .post(
            &members,
            json!({"name": "Zoé", "kind": "child", "diet": "vegetarian"}),
        )
        .await;
    assert_eq!(
        response.json["portions"],
        json!({"protein": 0.6, "starch": 1.0, "veg": 0.6, "fat": 0.6, "other": 0.6})
    );
    assert_eq!(response.json["diet"], "vegetarian");

    let list = alice.get(&members).await.json;
    let names: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Léo", "Zoé"]);

    let defaults = alice.get("/api/v1/portions/defaults").await.json;
    assert_eq!(defaults["teen"]["starch"], 2.0);
}

#[sqlx::test]
async fn modifier_bareme_presence_et_exclusions(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = alice_and_home(&app).await;
    let leo = alice
        .post(
            &format!("/api/v1/households/{home}/members"),
            json!({"name": "Léo", "kind": "teen"}),
        )
        .await
        .json;
    let url = format!(
        "/api/v1/households/{home}/members/{}",
        leo["id"].as_str().unwrap()
    );
    let peanut = ingredient_id(&mut alice, "peanut").await;
    let mushroom = ingredient_id(&mut alice, "mushroom").await;

    // Ado absent le midi sauf mercredi et samedi.
    let lunch = [false, false, true, false, false, true, false];
    let update = json!({
        "name": "Léo",
        "kind": "teen",
        "diet": "omnivore",
        "user_id": null,
        "portions": {"protein": 1.5, "starch": 2.0, "veg": 0.5, "fat": 1.0, "other": 1.0},
        "presence": {"lunch": lunch, "dinner": vec![true; 7]},
        "exclusions": [
            {"ingredient_id": peanut, "kind": "allergy"},
            {"ingredient_id": mushroom, "kind": "dislike"},
        ],
    });
    let response = alice.put(&url, update.clone()).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let read = alice.get(&url).await.json;
    assert_eq!(read["portions"], update["portions"]);
    assert_eq!(read["presence"], update["presence"]);
    assert_eq!(read["exclusions"], update["exclusions"]);

    // Changer le type ne réécrit pas un barème réglé à la main.
    let mut as_adult = update.clone();
    as_adult["kind"] = json!("adult");
    let response = alice.put(&url, as_adult).await;
    assert_eq!(response.json["kind"], "adult");
    assert_eq!(response.json["portions"], update["portions"]);

    let mut bad = update.clone();
    bad["presence"]["lunch"] = json!([true, true]);
    assert_eq!(
        alice.put(&url, bad).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut bad = update.clone();
    bad["portions"]["starch"] = json!(9.0);
    assert_eq!(
        alice.put(&url, bad).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut bad = update.clone();
    bad["exclusions"] = json!([{"ingredient_id": Uuid::now_v7(), "kind": "allergy"}]);
    assert_eq!(
        alice.put(&url, bad).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut bad = update.clone();
    bad["exclusions"] = json!([
        {"ingredient_id": peanut, "kind": "allergy"},
        {"ingredient_id": peanut, "kind": "dislike"},
    ]);
    assert_eq!(
        alice.put(&url, bad).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut bad = update.clone();
    bad["kind"] = json!("dragon");
    assert!(alice.put(&url, bad).await.status.is_client_error());

    assert_eq!(alice.delete(&url).await.status, StatusCode::NO_CONTENT);
    assert_eq!(alice.get(&url).await.status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn associer_un_compte_a_un_membre(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = alice_and_home(&app).await;
    let alice_id = alice.me().await["id"].clone();
    let members = format!("/api/v1/households/{home}/members");

    let response = alice
        .post(
            &members,
            json!({"name": "Alice", "kind": "adult", "user_id": alice_id}),
        )
        .await;
    assert_eq!(response.json["user_id"], alice_id);

    // Un compte, un seul membre par foyer.
    let response = alice
        .post(
            &members,
            json!({"name": "Alice bis", "kind": "adult", "user_id": alice_id}),
        )
        .await;
    assert_eq!(response.status, StatusCode::CONFLICT);

    // Un compte hors du foyer ne peut pas être associé.
    let response = alice
        .post(
            &members,
            json!({"name": "X", "kind": "adult", "user_id": Uuid::now_v7()}),
        )
        .await;
    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test]
async fn equipements_du_foyer(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = alice_and_home(&app).await;

    let catalog = alice.get("/api/v1/equipment").await.json;
    let cookeo = catalog
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["code"] == "cookeo_6l")
        .unwrap();
    assert_eq!(cookeo["rules"]["max_fill_l"], 4);
    assert_eq!(cookeo["name_key"], "equipment.cookeo_6l");

    let url = format!("/api/v1/households/{home}/equipment");
    assert_eq!(alice.get(&url).await.json, json!({"codes": []}));
    let response = alice
        .put(&url, json!({"codes": ["oven", "cookeo_6l", "oven"]}))
        .await;
    assert_eq!(response.json, json!({"codes": ["cookeo_6l", "oven"]}));
    let response = alice.put(&url, json!({"codes": ["teleporteur"]})).await;
    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        alice.get(&url).await.json,
        json!({"codes": ["cookeo_6l", "oven"]})
    );
}

#[sqlx::test]
async fn referentiel_d_ingredients(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, _) = alice_and_home(&app).await;
    let list = alice.get("/api/v1/ingredients").await.json;
    let peanut = list
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["code"] == "peanut")
        .unwrap();
    assert_eq!(peanut["name"], "Arachide");
    assert_eq!(peanut.get("name_key"), None);
    assert_eq!(peanut["aisle"], "grocery");
    assert!(list.as_array().unwrap().len() >= 40);
}

#[sqlx::test]
async fn un_autre_foyer_est_invisible(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Open).await;
    let (mut alice, home) = alice_and_home(&app).await;
    let leo = alice
        .post(
            &format!("/api/v1/households/{home}/members"),
            json!({"name": "Léo", "kind": "teen"}),
        )
        .await
        .json;
    let mut eve = app.browser();
    eve.login("eve@example.org").await;
    eve.household("Chez Eve").await;

    for url in [
        format!("/api/v1/households/{home}"),
        format!("/api/v1/households/{home}/members"),
        format!(
            "/api/v1/households/{home}/members/{}",
            leo["id"].as_str().unwrap()
        ),
        format!("/api/v1/households/{home}/equipment"),
    ] {
        assert_eq!(eve.get(&url).await.status, StatusCode::NOT_FOUND, "{url}");
    }
    let response = eve
        .put(
            &format!("/api/v1/households/{home}/equipment"),
            json!({"codes": ["oven"]}),
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert_eq!(
        eve.delete(&format!("/api/v1/households/{home}"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
async fn invitation_fait_entrer_dans_le_foyer(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = alice_and_home(&app).await;
    let token = alice.invite(home).await;

    // Avant connexion, la page de l'invitation sait qui invite et où.
    let mut bob = app.browser();
    let info = bob.get(&format!("/api/v1/invites/{token}")).await.json;
    assert_eq!(
        info,
        json!({"household_name": "Maison", "invited_by": "alice", "status": "pending", "joined_household_id": null})
    );
    assert_eq!(
        alice
            .get(&format!("/api/v1/households/{home}/invites"))
            .await
            .json
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // Inscriptions fermées, mais l'invitation permet de créer le compte.
    assert_eq!(
        bob.login_with("bob@example.org", Some(&token)).await,
        format!("/invitation/{token}")
    );
    let me = bob.me().await;
    assert_eq!(
        me["households"],
        json!([{"id": home, "name": "Maison", "role": "member"}])
    );
    let info = bob.get(&format!("/api/v1/invites/{token}")).await.json;
    assert_eq!(info["status"], "used");
    assert_eq!(info["joined_household_id"], json!(home));

    // Usage unique : l'invitation ne fait pas entrer quelqu'un d'autre.
    let mut eve = app.browser();
    assert_eq!(
        eve.login_with("eve@example.org", Some(&token)).await,
        "/connexion?erreur=inscriptions-fermees"
    );
    assert_eq!(
        alice
            .get(&format!("/api/v1/households/{home}/invites"))
            .await
            .json,
        json!([])
    );

    // Bob, membre, modifie la cuisine du foyer.
    let response = bob
        .post(
            &format!("/api/v1/households/{home}/members"),
            json!({"name": "Léo", "kind": "teen"}),
        )
        .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let response = bob
        .put(
            &format!("/api/v1/households/{home}/equipment"),
            json!({"codes": ["hob"]}),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK);
}

#[sqlx::test]
async fn invitation_acceptee_par_un_compte_existant(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Open).await;
    let (mut alice, home) = alice_and_home(&app).await;
    let mut bob = app.browser();
    bob.login("bob@example.org").await;

    let token = alice.invite(home).await;
    let response = bob
        .post(&format!("/api/v1/invites/{token}/accept"), json!(null))
        .await;
    assert_eq!(response.json, json!({"household_id": home}));
    assert_eq!(bob.me().await["households"][0]["role"], "member");

    let response = bob
        .post(&format!("/api/v1/invites/{token}/accept"), json!(null))
        .await;
    assert_eq!(response.status, StatusCode::GONE);
    assert_eq!(response.json["error"], "invite_invalid");
}

#[sqlx::test]
async fn invitation_expiree_ou_annulee(db: PgPool) {
    let app = TestApp::new(db.clone(), SignupPolicy::Closed).await;
    let (mut alice, home) = alice_and_home(&app).await;

    let token = alice.invite(home).await;
    sqlx::query("UPDATE household_invites SET expires_at = now() - interval '1 minute'")
        .execute(&db)
        .await
        .unwrap();
    let mut bob = app.browser();
    assert_eq!(
        bob.get(&format!("/api/v1/invites/{token}")).await.json["status"],
        "expired"
    );
    assert_eq!(
        bob.login_with("bob@example.org", Some(&token)).await,
        "/connexion?erreur=inscriptions-fermees"
    );

    let token = alice.invite(home).await;
    let pending = alice
        .get(&format!("/api/v1/households/{home}/invites"))
        .await
        .json;
    let invite_id = pending[0]["id"].as_str().unwrap();
    assert_eq!(
        alice
            .delete(&format!("/api/v1/households/{home}/invites/{invite_id}"))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        bob.get(&format!("/api/v1/invites/{token}")).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        bob.get("/api/v1/invites/pas-un-jeton!").await.status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
async fn roles_et_proprietaires(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = alice_and_home(&app).await;
    let mut bob = bob_joins(&app, &mut alice, home).await;
    let alice_id = alice.me().await["id"].as_str().unwrap().to_owned();
    let bob_id = bob.me().await["id"].as_str().unwrap().to_owned();
    let users = format!("/api/v1/households/{home}/users");

    // Un membre ne gère pas les comptes.
    assert_eq!(bob.invite_status(home).await, StatusCode::FORBIDDEN);
    let response = bob
        .patch(&format!("{users}/{alice_id}"), json!({"role": "member"}))
        .await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    assert_eq!(
        bob.delete(&format!("/api/v1/households/{home}"))
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    // Le dernier propriétaire ne peut ni être rétrogradé ni partir.
    let response = alice
        .patch(&format!("{users}/{alice_id}"), json!({"role": "member"}))
        .await;
    assert_eq!(response.status, StatusCode::CONFLICT);
    assert_eq!(response.json["error"], "last_owner");
    assert_eq!(
        alice.delete(&format!("{users}/{alice_id}")).await.status,
        StatusCode::CONFLICT
    );

    // Alice rend Bob propriétaire, puis se rétrograde elle-même.
    let response = alice
        .patch(&format!("{users}/{bob_id}"), json!({"role": "owner"}))
        .await;
    assert_eq!(response.status, StatusCode::OK);
    let roles: Vec<&str> = response.json["users"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["owner", "owner"]);
    let response = alice
        .patch(&format!("{users}/{alice_id}"), json!({"role": "member"}))
        .await;
    assert_eq!(response.json["role"], "member");
    assert_eq!(bob.invite_status(home).await, StatusCode::CREATED);

    // Alice part d'elle-même ; le foyer ne lui est plus visible.
    assert_eq!(
        alice.delete(&format!("{users}/{alice_id}")).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        alice
            .get(&format!("/api/v1/households/{home}"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(alice.me().await["households"], json!([]));
}

#[sqlx::test]
async fn proprietaire_retire_un_compte(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = alice_and_home(&app).await;
    let mut bob = bob_joins(&app, &mut alice, home).await;
    let bob_id = bob.me().await["id"].clone();
    let members = format!("/api/v1/households/{home}/members");
    let member = alice
        .post(
            &members,
            json!({"name": "Bob", "kind": "adult", "user_id": bob_id}),
        )
        .await
        .json;

    let response = alice
        .delete(&format!(
            "/api/v1/households/{home}/users/{}",
            bob_id.as_str().unwrap()
        ))
        .await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert_eq!(bob.get(&members).await.status, StatusCode::NOT_FOUND);
    // Le membre reste (il mange toujours ici), sans compte associé.
    let member = alice
        .get(&format!("{members}/{}", member["id"].as_str().unwrap()))
        .await
        .json;
    assert_eq!(member["user_id"], Value::Null);
}

#[sqlx::test]
async fn supprimer_le_foyer(db: PgPool) {
    let app = TestApp::new(db, SignupPolicy::Closed).await;
    let (mut alice, home) = alice_and_home(&app).await;
    alice
        .post(
            &format!("/api/v1/households/{home}/members"),
            json!({"name": "Léo", "kind": "teen"}),
        )
        .await;
    assert_eq!(
        alice
            .delete(&format!("/api/v1/households/{home}"))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(alice.me().await["households"], json!([]));
}

trait InviteStatus {
    async fn invite_status(&mut self, home: Uuid) -> StatusCode;
}

impl InviteStatus for Browser {
    async fn invite_status(&mut self, home: Uuid) -> StatusCode {
        self.post(&format!("/api/v1/households/{home}/invites"), Value::Null)
            .await
            .status
    }
}
