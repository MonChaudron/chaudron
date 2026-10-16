//! Fournisseur OAuth factice : remplace Amazon et Google en dev (`AUTH_FAKE=1`) et dans les tests.
//!
//! Même protocole que les vrais (code d'autorisation, PKCE S256, profil), sans compte ni mot de passe :
//! on choisit un nom et une adresse. Le serveur passe donc par exactement le même code qu'en production.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use axum::{
    Form, Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;

use crate::crypto::{pkce_challenge, random_token};

pub const CLIENT_ID: &str = "chaudron-dev";
pub const CLIENT_SECRET: &str = "chaudron-dev";

#[derive(Clone, Default)]
pub struct FakeOAuth {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    codes: HashMap<String, Grant>,
    tokens: HashMap<String, FakeUser>,
}

struct Grant {
    user: FakeUser,
    redirect_uri: String,
    challenge: String,
}

#[derive(Clone)]
struct FakeUser {
    name: String,
    email: String,
}

impl FakeOAuth {
    /// Routes `/authorize`, `/token`, `/userinfo`, à monter sous un préfixe.
    pub fn router<S>(&self) -> Router<S> {
        Router::new()
            .route("/authorize", get(authorize))
            .route("/token", post(token))
            .route("/userinfo", get(userinfo))
            .with_state(self.clone())
    }
}

#[derive(Deserialize)]
struct AuthorizeQuery {
    client_id: String,
    redirect_uri: String,
    state: String,
    code_challenge: String,
    code_challenge_method: String,
    scope: Option<String>,
    response_type: Option<String>,
    /// Rempli par le formulaire (ou directement par les tests).
    email: Option<String>,
    name: Option<String>,
}

async fn authorize(State(fake): State<FakeOAuth>, Query(q): Query<AuthorizeQuery>) -> Response {
    if q.client_id != CLIENT_ID || q.code_challenge_method != "S256" {
        return (
            StatusCode::BAD_REQUEST,
            "client_id ou code_challenge_method",
        )
            .into_response();
    }
    let Some(email) = q.email.as_deref().map(str::trim).filter(|e| !e.is_empty()) else {
        return Html(form(&q)).into_response();
    };
    let name = q
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| email.split('@').next().unwrap_or(email));
    let code = random_token();
    fake.inner.lock().unwrap().codes.insert(
        code.clone(),
        Grant {
            user: FakeUser {
                name: name.to_owned(),
                email: email.to_lowercase(),
            },
            redirect_uri: q.redirect_uri.clone(),
            challenge: q.code_challenge.clone(),
        },
    );
    let query = serde_urlencoded::to_string([("code", code.as_str()), ("state", &q.state)])
        .expect("paramètres de retour");
    Redirect::to(&format!("{}?{query}", q.redirect_uri)).into_response()
}

fn form(q: &AuthorizeQuery) -> String {
    let hidden = [
        ("client_id", q.client_id.as_str()),
        ("redirect_uri", &q.redirect_uri),
        ("state", &q.state),
        ("code_challenge", &q.code_challenge),
        ("code_challenge_method", &q.code_challenge_method),
        ("scope", q.scope.as_deref().unwrap_or("")),
        (
            "response_type",
            q.response_type.as_deref().unwrap_or("code"),
        ),
    ]
    .iter()
    .map(|(k, v)| format!(r#"<input type="hidden" name="{k}" value="{}">"#, escape(v)))
    .collect::<String>();
    format!(
        r#"<!doctype html>
<html lang="fr"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Connexion factice</title>
<style>body{{font-family:system-ui,sans-serif;max-width:24rem;margin:3rem auto;padding:0 1rem}}label,input,button{{display:block;width:100%;margin:.4rem 0;font-size:1rem}}</style>
</head><body>
<h1>Connexion factice</h1>
<p>Fournisseur de développement : aucun mot de passe. La même adresse redonne le même compte.</p>
<form method="get" action="authorize">{hidden}
<label>Nom <input name="name" placeholder="Alice"></label>
<label>Adresse e-mail <input name="email" type="email" required placeholder="alice@example.org"></label>
<button>Se connecter</button>
</form></body></html>"#
    )
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[derive(Deserialize)]
struct TokenForm {
    grant_type: String,
    code: String,
    redirect_uri: String,
    client_id: String,
    client_secret: String,
    code_verifier: String,
}

async fn token(State(fake): State<FakeOAuth>, Form(f): Form<TokenForm>) -> Response {
    let invalid = || {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_grant"})),
        )
            .into_response()
    };
    if f.grant_type != "authorization_code"
        || f.client_id != CLIENT_ID
        || f.client_secret != CLIENT_SECRET
    {
        return invalid();
    }
    let mut inner = fake.inner.lock().unwrap();
    // Un code ne sert qu'une fois, même si la suite échoue.
    let Some(grant) = inner.codes.remove(&f.code) else {
        return invalid();
    };
    if grant.redirect_uri != f.redirect_uri || grant.challenge != pkce_challenge(&f.code_verifier) {
        return invalid();
    }
    let access_token = random_token();
    inner.tokens.insert(access_token.clone(), grant.user);
    Json(json!({"access_token": access_token, "token_type": "Bearer", "expires_in": 3600}))
        .into_response()
}

async fn userinfo(State(fake): State<FakeOAuth>, headers: HeaderMap) -> Response {
    let user = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .and_then(|t| fake.inner.lock().unwrap().tokens.get(t).cloned());
    match user {
        Some(user) => Json(json!({
            "sub": format!("fake-{}", user.email),
            "name": user.name,
            "email": user.email,
            "email_verified": true,
        }))
        .into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}
