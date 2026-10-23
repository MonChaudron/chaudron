//! Client HTTP(S) sortant : fournisseurs OAuth, sources des prix estimés.
//!
//! hyper (déjà présent via axum) + rustls/ring, avec les racines Mozilla embarquées dans le binaire :
//! l'image `FROM scratch` n'a pas de magasin de certificats.

use std::time::Duration;

use axum::body::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::{
    Request,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderValue, USER_AGENT},
};
use hyper_rustls::HttpsConnector;
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
use serde_json::Value;

pub type HttpClient = Client<HttpsConnector<HttpConnector>, Full<Bytes>>;

const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BODY: usize = 256 * 1024;
/// Qui appelle : les services publics (Open Prices, INSEE) demandent qu'on se présente.
const AGENT: &str = concat!(
    "Chaudron/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/MonChaudron/chaudron)"
);

pub fn client() -> HttpClient {
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_webpki_roots()
        // http seulement pour le fournisseur factice (dev, tests) ; les vrais fournisseurs sont en https.
        .https_or_http()
        .enable_http1()
        .build();
    Client::builder(TokioExecutor::new()).build(https)
}

/// POST d'un formulaire, réponse JSON attendue.
pub async fn post_form(client: &HttpClient, url: &str, form: String) -> Result<Value, String> {
    let request = Request::post(url)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(ACCEPT, "application/json")
        .body(Full::new(Bytes::from(form)))
        .map_err(|err| err.to_string())?;
    let body = send(client, request).await?;
    serde_json::from_slice(&body).map_err(|err| err.to_string())
}

/// GET, avec un jeton `Bearer` si donné, réponse JSON attendue.
pub async fn get_json(
    client: &HttpClient,
    url: &str,
    bearer: Option<&str>,
) -> Result<Value, String> {
    let mut request = Request::get(url).header(ACCEPT, "application/json");
    if let Some(token) = bearer {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = request
        .body(Full::default())
        .map_err(|err| err.to_string())?;
    let body = send(client, request).await?;
    serde_json::from_slice(&body).map_err(|err| err.to_string())
}

/// GET, réponse texte (le XML SDMX de l'INSEE).
pub async fn get_text(client: &HttpClient, url: &str) -> Result<String, String> {
    let request = Request::get(url)
        .body(Full::default())
        .map_err(|err| err.to_string())?;
    let body = send(client, request).await?;
    String::from_utf8(body.to_vec()).map_err(|err| err.to_string())
}

async fn send(client: &HttpClient, mut request: Request<Full<Bytes>>) -> Result<Bytes, String> {
    request
        .headers_mut()
        .insert(USER_AGENT, HeaderValue::from_static(AGENT));
    let exchange = async {
        let response = client
            .request(request)
            .await
            .map_err(|err| err.to_string())?;
        let status = response.status();
        let body = Limited::new(response.into_body(), MAX_BODY)
            .collect()
            .await
            .map_err(|err| err.to_string())?
            .to_bytes();
        if !status.is_success() {
            let extract = String::from_utf8_lossy(&body[..body.len().min(300)]).into_owned();
            return Err(format!("HTTP {status} : {extract}"));
        }
        Ok(body)
    };
    tokio::time::timeout(TIMEOUT, exchange)
        .await
        .map_err(|_| "délai dépassé".to_owned())?
}
