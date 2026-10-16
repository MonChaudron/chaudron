//! Outils communs des tests d'intégration : appli de test, fournisseur OAuth factice sur un vrai port,
//! « navigateur » qui garde les cookies.

#![allow(dead_code)]

use std::{collections::BTreeMap, path::PathBuf, time::Duration};

use axum::{
    Router,
    body::Body,
    http::{
        HeaderMap, Request, StatusCode,
        header::{CONTENT_TYPE, COOKIE, LOCATION, ORIGIN, SET_COOKIE},
    },
};
use chaudron_server::{
    AppState, app,
    auth::{fake::FakeOAuth, providers::Provider},
    config::{Config, SignupPolicy},
};
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;
use uuid::Uuid;

pub const PUBLIC_URL: &str = "http://localhost:28471";

pub fn web_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web")
}

pub fn config(fake_base: &str, signup: SignupPolicy) -> Config {
    Config {
        public_url: PUBLIC_URL.into(),
        providers: vec![Provider::fake(fake_base, fake_base)],
        signup,
        fake_oauth: false,
    }
}

/// Une base qui ne répond jamais : pour les routes qui n'en ont pas besoin, et pour le cas dégradé.
pub fn unreachable_db() -> PgPool {
    PgPoolOptions::new()
        .acquire_timeout(Duration::from_millis(200))
        .connect_lazy("postgres://chaudron@127.0.0.1:1/chaudron")
        .unwrap()
}

/// Appli sans fournisseur joignable, pour les tests qui ne se connectent pas.
pub fn offline_app(db: PgPool) -> Router {
    app(
        AppState::new(db, config("http://127.0.0.1:1", SignupPolicy::Closed)),
        &web_dir(),
    )
}

/// Fournisseur factice servi sur un port local : le serveur testé lui parle en vrai HTTP.
pub struct Fake {
    pub base: String,
    oauth: FakeOAuth,
}

impl Fake {
    pub async fn start() -> Self {
        let oauth = FakeOAuth::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let router: Router = oauth.router();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { base, oauth }
    }
}

pub struct TestApp {
    pub app: Router,
    pub fake: Fake,
}

impl TestApp {
    pub async fn new(db: PgPool, signup: SignupPolicy) -> Self {
        let fake = Fake::start().await;
        let app = app(AppState::new(db, config(&fake.base, signup)), &web_dir());
        Self { app, fake }
    }

    pub fn browser(&self) -> Browser {
        Browser {
            app: self.app.clone(),
            fake: self.fake.oauth.router(),
            fake_base: self.fake.base.clone(),
            cookies: BTreeMap::new(),
            origin: Some(PUBLIC_URL.into()),
        }
    }
}

pub struct Response {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub text: String,
    /// `Null` si le corps n'est pas du JSON.
    pub json: Value,
}

impl Response {
    pub fn location(&self) -> &str {
        self.headers
            .get(LOCATION)
            .expect("en-tête Location")
            .to_str()
            .unwrap()
    }

    pub fn set_cookies(&self) -> Vec<&str> {
        self.headers
            .get_all(SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap())
            .collect()
    }
}

/// Navigateur simplifié : garde les cookies et envoie l'en-tête `Origin` sur les écritures, comme un vrai.
pub struct Browser {
    app: Router,
    fake: Router,
    fake_base: String,
    pub cookies: BTreeMap<String, String>,
    pub origin: Option<String>,
}

impl Browser {
    pub async fn request(&mut self, method: &str, uri: &str, body: Option<Value>) -> Response {
        let mut request = Request::builder().method(method).uri(uri);
        if !self.cookies.is_empty() {
            let cookies: Vec<String> = self
                .cookies
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            request = request.header(COOKIE, cookies.join("; "));
        }
        if method != "GET"
            && let Some(origin) = &self.origin
        {
            request = request.header(ORIGIN, origin);
        }
        let body = match body {
            Some(json) => {
                request = request.header(CONTENT_TYPE, "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };
        let response = self
            .app
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        for value in response.headers().get_all(SET_COOKIE) {
            let value = value.to_str().unwrap();
            let (pair, attributes) = value.split_once(';').unwrap_or((value, ""));
            let (name, content) = pair.split_once('=').unwrap();
            if content.is_empty() || attributes.contains("Max-Age=0") {
                self.cookies.remove(name);
            } else {
                self.cookies.insert(name.into(), content.into());
            }
        }
        read(response).await
    }

    pub async fn get(&mut self, uri: &str) -> Response {
        self.request("GET", uri, None).await
    }

    pub async fn post(&mut self, uri: &str, body: Value) -> Response {
        self.request("POST", uri, Some(body)).await
    }

    pub async fn put(&mut self, uri: &str, body: Value) -> Response {
        self.request("PUT", uri, Some(body)).await
    }

    pub async fn patch(&mut self, uri: &str, body: Value) -> Response {
        self.request("PATCH", uri, Some(body)).await
    }

    pub async fn delete(&mut self, uri: &str) -> Response {
        self.request("DELETE", uri, None).await
    }

    /// Va chez le fournisseur factice : renvoie la redirection vers le callback (ou l'erreur).
    pub async fn authorize(&mut self, login_uri: &str, email: &str) -> String {
        let response = self.get(login_uri).await;
        assert_eq!(response.status, StatusCode::SEE_OTHER, "{}", response.text);
        let authorize = response.location().strip_prefix(&self.fake_base).unwrap();
        let email = serde_urlencoded::to_string([("email", email)]).unwrap();
        let response = read(
            self.fake
                .clone()
                .oneshot(
                    Request::get(format!("{authorize}&{email}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(response.status, StatusCode::SEE_OTHER, "{}", response.text);
        response
            .location()
            .strip_prefix(PUBLIC_URL)
            .unwrap()
            .to_owned()
    }

    /// Connexion complète ; renvoie où l'appli redirige à la fin (`/`, `/connexion?erreur=…`…).
    pub async fn login_with(&mut self, email: &str, invite: Option<&str>) -> String {
        let login = match invite {
            Some(token) => format!("/api/v1/auth/fake/login?invite={token}"),
            None => "/api/v1/auth/fake/login".into(),
        };
        let callback = self.authorize(&login, email).await;
        let response = self.get(&callback).await;
        assert_eq!(response.status, StatusCode::SEE_OTHER, "{}", response.text);
        response.location().to_owned()
    }

    pub async fn login(&mut self, email: &str) -> String {
        self.login_with(email, None).await
    }

    pub async fn me(&mut self) -> Value {
        let response = self.get("/api/v1/me").await;
        assert_eq!(response.status, StatusCode::OK, "{}", response.text);
        response.json
    }

    /// Crée un foyer et renvoie son identifiant.
    pub async fn household(&mut self, name: &str) -> Uuid {
        let response = self
            .post("/api/v1/households", serde_json::json!({ "name": name }))
            .await;
        assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
        response.json["id"].as_str().unwrap().parse().unwrap()
    }

    /// Crée une invitation et renvoie son jeton.
    pub async fn invite(&mut self, household: Uuid) -> String {
        let response = self
            .post(
                &format!("/api/v1/households/{household}/invites"),
                Value::Null,
            )
            .await;
        assert_eq!(response.status, StatusCode::CREATED, "{}", response.text);
        let url = response.json["url"].as_str().unwrap();
        url.strip_prefix(&format!("{PUBLIC_URL}/invitation/"))
            .unwrap()
            .to_owned()
    }
}

async fn read(response: axum::response::Response) -> Response {
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    let json = serde_json::from_str(&text).unwrap_or(Value::Null);
    Response {
        status,
        headers,
        text,
        json,
    }
}
