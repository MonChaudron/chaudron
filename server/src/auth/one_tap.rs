//! Google One Tap : la page reçoit de Google un jeton d'identité (JWT signé RS256) et nous l'envoie.
//!
//! Le jeton ne vient pas d'un échange direct avec Google : on vérifie sa signature avec les clés publiques de Google
//! (mises en cache une heure, rechargées si une clé inconnue apparaît), puis l'émetteur, l'audience et l'expiration.
//! `ring` suffit (déjà présent pour TLS) : pas de bibliothèque JWT.

use std::{
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::signature::{RSA_PKCS1_2048_8192_SHA256, RsaPublicKeyComponents};
use serde::Deserialize;
use serde_json::Value;

use super::providers::{Profile, Provider};
use crate::http::{self, HttpClient};

const KEYS_TTL: Duration = Duration::from_secs(3600);
/// Tolérance d'horloge sur l'expiration.
const CLOCK_SKEW_SECONDS: i64 = 60;

#[derive(Clone, Deserialize)]
struct Jwk {
    kid: String,
    n: String,
    e: String,
}

/// Clés publiques de Google, en cache.
#[derive(Default)]
pub struct GoogleKeys {
    cache: Mutex<Option<(Instant, Vec<Jwk>)>>,
}

impl GoogleKeys {
    async fn find(
        &self,
        client: &HttpClient,
        url: &str,
        kid: &str,
    ) -> Result<(String, String), String> {
        let cached = {
            let cache = self.cache.lock().unwrap();
            cache
                .as_ref()
                .filter(|(at, _)| at.elapsed() < KEYS_TTL)
                .and_then(|(_, keys)| keys.iter().find(|k| k.kid == kid).cloned())
        };
        let key = match cached {
            Some(key) => key,
            None => {
                // Cache vide, périmé, ou clé tournée chez Google : on recharge.
                let jwks = http::get_json(client, url, None).await?;
                let keys: Vec<Jwk> = serde_json::from_value(jwks["keys"].clone())
                    .map_err(|err| format!("clés Google illisibles : {err}"))?;
                let key = keys.iter().find(|k| k.kid == kid).cloned();
                *self.cache.lock().unwrap() = Some((Instant::now(), keys));
                key.ok_or("clé de signature inconnue")?
            }
        };
        Ok((key.n, key.e))
    }
}

#[derive(Deserialize)]
struct Header {
    alg: String,
    kid: String,
}

fn decode(part: &str) -> Result<Vec<u8>, String> {
    URL_SAFE_NO_PAD
        .decode(part)
        .map_err(|_| "jeton mal formé".to_owned())
}

/// Vérifie un jeton d'identité Google et en tire le profil.
pub async fn verify(
    keys: &GoogleKeys,
    client: &HttpClient,
    google: &Provider,
    credential: &str,
) -> Result<Profile, String> {
    let jwks_url = google.jwks_url.as_deref().ok_or("One Tap non configuré")?;
    let mut parts = credential.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err("jeton mal formé".into());
    };

    let head: Header =
        serde_json::from_slice(&decode(header)?).map_err(|_| "en-tête illisible".to_owned())?;
    if head.alg != "RS256" {
        return Err(format!("algorithme refusé : {}", head.alg));
    }
    let (n, e) = keys.find(client, jwks_url, &head.kid).await?;
    RsaPublicKeyComponents {
        n: decode(&n)?,
        e: decode(&e)?,
    }
    .verify(
        &RSA_PKCS1_2048_8192_SHA256,
        format!("{header}.{payload}").as_bytes(),
        &decode(signature)?,
    )
    .map_err(|_| "signature invalide".to_owned())?;

    let claims: Value =
        serde_json::from_slice(&decode(payload)?).map_err(|_| "contenu illisible".to_owned())?;
    let issuer = claims["iss"].as_str().unwrap_or_default();
    if issuer != "accounts.google.com" && issuer != "https://accounts.google.com" {
        return Err(format!("émetteur refusé : {issuer}"));
    }
    if claims["aud"].as_str() != Some(google.client_id.as_str()) {
        return Err("jeton destiné à une autre appli".into());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    if claims["exp"].as_i64().unwrap_or(0) + CLOCK_SKEW_SECONDS < now {
        return Err("jeton expiré".into());
    }

    let text = |key: &str| {
        claims[key]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    Ok(Profile {
        subject: text("sub").ok_or("jeton sans sub")?,
        name: text("name"),
        email: text("email"),
        // Booléen, ou "true" dans d'anciens jetons.
        email_verified: claims["email_verified"].as_bool().unwrap_or(false)
            || claims["email_verified"].as_str() == Some("true"),
    })
}
