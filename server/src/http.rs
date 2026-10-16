//! Client HTTP(S) sortant, pour parler aux fournisseurs OAuth.
//!
//! hyper (déjà présent via axum) + rustls/ring, avec les racines Mozilla embarquées dans le binaire :
//! l'image `FROM scratch` n'a pas de magasin de certificats.

use std::time::Duration;

use axum::body::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::{
    Request,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
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
    send(client, request).await
}

/// GET avec un jeton `Bearer`, réponse JSON attendue.
pub async fn get_json(client: &HttpClient, url: &str, bearer: &str) -> Result<Value, String> {
    let request = Request::get(url)
        .header(AUTHORIZATION, format!("Bearer {bearer}"))
        .header(ACCEPT, "application/json")
        .body(Full::default())
        .map_err(|err| err.to_string())?;
    send(client, request).await
}

async fn send(client: &HttpClient, request: Request<Full<Bytes>>) -> Result<Value, String> {
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
        serde_json::from_slice(&body).map_err(|err| err.to_string())
    };
    tokio::time::timeout(TIMEOUT, exchange)
        .await
        .map_err(|_| "délai dépassé".to_owned())?
}
