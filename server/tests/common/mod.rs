//! Outils communs des tests d'intégration : appli de test, fournisseur OAuth factice sur un vrai port,
//! « navigateur » qui garde les cookies.

#![allow(dead_code)]

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{
        HeaderMap, Request, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE, COOKIE, LOCATION, ORIGIN, SET_COOKIE},
    },
    response::{IntoResponse, Response as AxumResponse},
    routing::get,
};
use chaudron_server::{
    AppState, app,
    auth::{fake::FakeOAuth, providers::Provider},
    config::{Config, OAuthClient, PriceSources, SignupPolicy},
};
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;
use uuid::Uuid;

pub const PUBLIC_URL: &str = "http://localhost:28471";

/// Client OAuth de la liaison de compte Alexa, tel que déclaré dans `.env`.
pub const ALEXA_ID: &str = "alexa-skill";
pub const ALEXA_SECRET: &str = "secret-de-test";
pub const ALEXA_REDIRECT: &str = "https://layla.amazon.com/api/skill/link/M2TEST";

pub fn alexa_client() -> OAuthClient {
    OAuthClient {
        id: ALEXA_ID.into(),
        secret: ALEXA_SECRET.into(),
        name: "Alexa".into(),
        redirect_uris: vec![
            ALEXA_REDIRECT.into(),
            "https://pitangui.amazon.com/api/skill/link/M2TEST".into(),
        ],
    }
}

pub fn web_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/web")
}

pub fn config(fake_base: &str, signup: SignupPolicy) -> Config {
    Config {
        public_url: PUBLIC_URL.into(),
        providers: vec![Provider::fake(fake_base, fake_base)],
        signup,
        fake_oauth: false,
        dev_web_origin: None,
        oauth_clients: vec![alexa_client()],
        admin_emails: Vec::new(),
        prices: PriceSources {
            open_prices_url: format!("{fake_base}/prix/open-prices"),
            insee_url: format!("{fake_base}/prix/insee"),
            pause: Duration::ZERO,
        },
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
    /// Sources de prix factices (Open Prices, INSEE), réglées par le test.
    pub prices: FakePrices,
}

impl Fake {
    pub async fn start() -> Self {
        let oauth = FakeOAuth::default();
        let prices = FakePrices::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let router: Router = oauth.router().nest("/prix", prices.router());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self {
            base,
            oauth,
            prices,
        }
    }
}

/// Open Prices et l'INSEE factices : des relevés (au format de l'API Open Prices) et des séries (idbank → mois,
/// valeur), et une panne à la demande.
#[derive(Clone, Default)]
pub struct FakePrices(Arc<Mutex<PriceData>>);

#[derive(Default)]
struct PriceData {
    items: Vec<Value>,
    series: HashMap<String, Vec<(String, f64)>>,
    down: bool,
    /// Catégories qui répondent 500 (une panne ponctuelle).
    failing: Vec<String>,
    /// Open Prices répond 200 avec un corps sans relevés (une page de maintenance).
    garbled: bool,
}

impl FakePrices {
    /// Un relevé en vrac (`category_tag`), au kilo, en France sauf mention.
    pub fn raw(&self, category: &str, euros: f64, days_ago: i64, country: &str, labels: &[&str]) {
        self.0.lock().unwrap().items.push(serde_json::json!({
            "price": euros, "price_per": "KILOGRAM", "price_is_discounted": false, "date": date_ago(days_ago),
            "category_tag": category, "labels_tags": labels, "location": {"osm_address_country": country},
        }));
    }

    /// Un relevé de produit à code-barres de cette catégorie, de `grams` grammes.
    pub fn product(&self, category: &str, euros: f64, grams: f64, days_ago: i64) {
        self.0.lock().unwrap().items.push(serde_json::json!({
            "price": euros, "price_is_discounted": false, "date": date_ago(days_ago), "category_tag": null,
            "product": {"categories_tags": ["en:food", category], "product_quantity": grams,
                        "product_quantity_unit": "g", "labels_tags": []},
            "location": {"osm_address_country": "France"},
        }));
    }

    /// Une série INSEE (mois `AAAA-MM`, valeur), du plus récent au plus ancien.
    pub fn series(&self, idbank: &str, points: &[(&str, f64)]) {
        self.0.lock().unwrap().series.insert(
            idbank.to_owned(),
            points.iter().map(|(m, v)| ((*m).to_owned(), *v)).collect(),
        );
    }

    /// Les deux sources répondent 500.
    pub fn set_down(&self, down: bool) {
        self.0.lock().unwrap().down = down;
    }

    /// Open Prices répond 200, mais avec un corps illisible.
    pub fn set_garbled(&self, garbled: bool) {
        self.0.lock().unwrap().garbled = garbled;
    }

    /// Open Prices répond 500 pour cette catégorie seulement.
    pub fn fail_category(&self, category: &str) {
        self.0.lock().unwrap().failing.push(category.to_owned());
    }

    fn router(&self) -> Router {
        Router::new()
            .route("/open-prices/prices", get(fake_open_prices))
            .route("/insee/data/SERIES_BDM/{idbank}", get(fake_insee))
            .with_state(self.clone())
    }
}

pub fn date_ago(days: i64) -> String {
    (time::OffsetDateTime::now_utc().date() - time::Duration::days(days)).to_string()
}

async fn fake_open_prices(
    State(fake): State<FakePrices>,
    Query(q): Query<HashMap<String, String>>,
) -> AxumResponse {
    let data = fake.0.lock().unwrap();
    let asked = [
        q.get("category_tag"),
        q.get("product__categories_tags__contains"),
    ];
    if data.down || asked.iter().flatten().any(|tag| data.failing.contains(tag)) {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if data.garbled {
        return Json(serde_json::json!({"detail": "Service en maintenance"})).into_response();
    }
    let since = q.get("date__gte").cloned().unwrap_or_default();
    let matching: Vec<&Value> = data
        .items
        .iter()
        .filter(|i| i["date"].as_str().unwrap_or_default() >= since.as_str())
        .filter(|i| {
            match (
                q.get("category_tag"),
                q.get("product__categories_tags__contains"),
            ) {
                (Some(tag), _) => i["category_tag"] == tag.as_str(),
                (None, Some(tag)) => i["product"]["categories_tags"]
                    .as_array()
                    .is_some_and(|tags| tags.iter().any(|t| t == tag.as_str())),
                (None, None) => false,
            }
        })
        .collect();
    let size: usize = q.get("size").and_then(|s| s.parse().ok()).unwrap_or(50);
    let page: usize = q.get("page").and_then(|s| s.parse().ok()).unwrap_or(1);
    let pages = matching.len().div_ceil(size).max(1);
    let items: Vec<&Value> = matching
        .into_iter()
        .skip((page - 1) * size)
        .take(size)
        .collect();
    Json(serde_json::json!({"items": items, "page": page, "size": size, "pages": pages}))
        .into_response()
}

async fn fake_insee(State(fake): State<FakePrices>, Path(idbank): Path<String>) -> AxumResponse {
    let data = fake.0.lock().unwrap();
    if data.down {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let obs: String = data
        .series
        .get(&idbank)
        .into_iter()
        .flatten()
        .map(|(m, v)| format!(r#"<Obs TIME_PERIOD="{m}" OBS_VALUE="{v}" OBS_STATUS="A"/>"#))
        .collect();
    format!(
        r#"<?xml version='1.0' encoding='UTF-8'?><message:StructureSpecificData><message:DataSet><Series IDBANK="{idbank}">{obs}</Series></message:DataSet></message:StructureSpecificData>"#
    )
    .into_response()
}

pub struct TestApp {
    pub app: Router,
    pub fake: Fake,
}

impl TestApp {
    pub async fn new(db: PgPool, signup: SignupPolicy) -> Self {
        Self::with(db, |fake_base| config(fake_base, signup)).await
    }

    /// Appli avec une configuration sur mesure (reçoit l'adresse du fournisseur factice).
    pub async fn with(db: PgPool, make: impl FnOnce(&str) -> Config) -> Self {
        let fake = Fake::start().await;
        let app = app(AppState::new(db, make(&fake.base)), &web_dir());
        Self { app, fake }
    }

    /// Appel de serveur à serveur (Amazon qui échange un code) : formulaire, authentification Basic, ni cookie ni
    /// `Origin`.
    pub async fn form(
        &self,
        uri: &str,
        fields: &[(&str, &str)],
        basic: Option<(&str, &str)>,
    ) -> Response {
        use base64::Engine;
        let mut request = Request::builder()
            .method("POST")
            .uri(uri)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded");
        if let Some((user, password)) = basic {
            let credentials =
                base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"));
            request = request.header(AUTHORIZATION, format!("Basic {credentials}"));
        }
        let body = serde_urlencoded::to_string(fields).unwrap();
        read(
            self.app
                .clone()
                .oneshot(request.body(Body::from(body)).unwrap())
                .await
                .unwrap(),
        )
        .await
    }

    pub fn browser(&self) -> Browser {
        Browser {
            app: self.app.clone(),
            fake: self.fake.oauth.router(),
            fake_base: self.fake.base.clone(),
            cookies: BTreeMap::new(),
            origin: Some(PUBLIC_URL.into()),
            bearer: None,
        }
    }

    /// Client d'API sans navigateur (Claude, un script) : jeton `Bearer`, ni cookie ni `Origin`.
    pub fn api_client(&self, token: &str) -> Browser {
        Browser {
            origin: None,
            bearer: Some(token.into()),
            ..self.browser()
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
    pub bearer: Option<String>,
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
        if let Some(token) = &self.bearer {
            request = request.header(AUTHORIZATION, format!("Bearer {token}"));
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

    /// Ouvre une réponse sans lire son corps (flux SSE) : statut, en-têtes et corps à lire au fil de l'eau.
    pub async fn open(&mut self, uri: &str) -> axum::response::Response {
        let mut request = Request::builder().method("GET").uri(uri);
        if !self.cookies.is_empty() {
            let cookies: Vec<String> = self
                .cookies
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            request = request.header(COOKIE, cookies.join("; "));
        }
        self.app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
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

/// Lie Alexa au foyer comme le ferait Amazon (écran « Autoriser », puis échange du code) : le jeton d'accès.
pub async fn link_alexa(app: &TestApp, user: &mut Browser, home: Uuid) -> String {
    let response = user
        .post(
            "/api/v1/oauth/authorize",
            serde_json::json!({"client_id": ALEXA_ID, "redirect_uri": ALEXA_REDIRECT, "state": "s", "household_id": home}),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text);
    let target = response.json["redirect_to"].as_str().unwrap();
    let (_, query) = target.split_once('?').unwrap();
    let params: Vec<(String, String)> = serde_urlencoded::from_str(query).unwrap();
    let code = params.iter().find(|(k, _)| k == "code").unwrap().1.clone();
    let token = app
        .form(
            "/api/v1/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", ALEXA_REDIRECT),
            ],
            Some((ALEXA_ID, ALEXA_SECRET)),
        )
        .await;
    assert_eq!(token.status, StatusCode::OK, "{}", token.text);
    token.json["access_token"].as_str().unwrap().to_owned()
}
