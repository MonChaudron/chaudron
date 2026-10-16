//! Fournisseurs OAuth : un seul flux générique (code d'autorisation + PKCE, puis profil), décrit par une
//! déclaration par fournisseur. En ajouter un = une entrée ici et deux variables dans `.env`.

use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;

use crate::http::{self, HttpClient};

/// Fournisseur d'identité.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    Google,
    Amazon,
    /// Fournisseur factice de dev et de test.
    Fake,
}

impl ProviderId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Amazon => "amazon",
            Self::Fake => "fake",
        }
    }

    pub fn env_prefix(self) -> &'static str {
        match self {
            Self::Google => "GOOGLE",
            Self::Amazon => "AMAZON",
            Self::Fake => "FAKE",
        }
    }
}

pub struct Provider {
    pub id: ProviderId,
    pub client_id: String,
    pub client_secret: String,
    pub authorize_url: String,
    pub token_url: String,
    pub userinfo_url: String,
    pub scope: &'static str,
    /// Paramètres ajoutés à l'URL d'autorisation.
    pub extra_params: &'static [(&'static str, &'static str)],
}

/// Profil normalisé, lu chez le fournisseur après l'échange du code.
#[derive(Debug)]
pub struct Profile {
    /// Identifiant stable chez le fournisseur.
    pub subject: String,
    pub name: Option<String>,
    pub email: Option<String>,
    pub email_verified: bool,
}

impl Profile {
    pub fn verified_email(&self) -> Option<&str> {
        self.email.as_deref().filter(|_| self.email_verified)
    }
}

impl Provider {
    pub fn new(id: ProviderId, client_id: String, client_secret: String) -> Self {
        match id {
            // OpenID Connect ; `select_account` : choisir le compte même si un seul est ouvert.
            ProviderId::Google => Self {
                id,
                client_id,
                client_secret,
                authorize_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
                token_url: "https://oauth2.googleapis.com/token".into(),
                userinfo_url: "https://openidconnect.googleapis.com/v1/userinfo".into(),
                scope: "openid email profile",
                extra_params: &[("prompt", "select_account")],
            },
            // Login with Amazon : OAuth 2 (pas OpenID Connect), profil sur api.amazon.com.
            ProviderId::Amazon => Self {
                id,
                client_id,
                client_secret,
                authorize_url: "https://www.amazon.com/ap/oa".into(),
                token_url: "https://api.amazon.com/auth/o2/token".into(),
                userinfo_url: "https://api.amazon.com/user/profile".into(),
                scope: "profile",
                extra_params: &[],
            },
            ProviderId::Fake => panic!("Provider::fake pour le fournisseur factice"),
        }
    }

    /// `public_base` : vu par le navigateur ; `internal_base` : vu par le serveur.
    pub fn fake(public_base: &str, internal_base: &str) -> Self {
        Self {
            id: ProviderId::Fake,
            client_id: super::fake::CLIENT_ID.into(),
            client_secret: super::fake::CLIENT_SECRET.into(),
            authorize_url: format!("{public_base}/authorize"),
            token_url: format!("{internal_base}/token"),
            userinfo_url: format!("{internal_base}/userinfo"),
            scope: "openid email profile",
            extra_params: &[],
        }
    }

    pub fn authorization_url(&self, redirect_uri: &str, state: &str, challenge: &str) -> String {
        let mut params = vec![
            ("response_type", "code"),
            ("client_id", self.client_id.as_str()),
            ("redirect_uri", redirect_uri),
            ("scope", self.scope),
            ("state", state),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
        ];
        params.extend_from_slice(self.extra_params);
        let query = serde_urlencoded::to_string(params).expect("paramètres d'autorisation");
        format!("{}?{query}", self.authorize_url)
    }

    /// Échange le code contre un jeton, puis lit le profil. On parle directement au fournisseur en TLS :
    /// pas besoin de vérifier de signature (OpenID Connect Core, 3.1.3.7).
    pub async fn fetch_profile(
        &self,
        client: &HttpClient,
        code: &str,
        redirect_uri: &str,
        verifier: &str,
    ) -> Result<Profile, String> {
        let form = serde_urlencoded::to_string([
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
            ("code_verifier", verifier),
        ])
        .map_err(|err| err.to_string())?;
        let token = http::post_form(client, &self.token_url, form).await?;
        let access_token = token["access_token"]
            .as_str()
            .ok_or("réponse sans access_token")?;
        let info = http::get_json(client, &self.userinfo_url, access_token).await?;
        self.profile(&info).ok_or_else(|| "profil incomplet".into())
    }

    fn profile(&self, info: &Value) -> Option<Profile> {
        let text = |key: &str| {
            info[key]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        match self.id {
            ProviderId::Google | ProviderId::Fake => Some(Profile {
                subject: text("sub")?,
                name: text("name"),
                email: text("email"),
                email_verified: info["email_verified"].as_bool().unwrap_or(false),
            }),
            // Amazon ne dit pas si l'adresse est vérifiée : on ne s'en sert pas pour autoriser une
            // inscription (le premier compte et les invitations restent possibles).
            ProviderId::Amazon => Some(Profile {
                subject: text("user_id")?,
                name: text("name"),
                email: text("email"),
                email_verified: false,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_d_autorisation_google() {
        let google = Provider::new(ProviderId::Google, "id".into(), "secret".into());
        let url = google.authorization_url("https://c.example/cb", "s1", "c1");
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?response_type=code&client_id=id&redirect_uri=https%3A%2F%2Fc.example%2Fcb&scope=openid+email+profile&state=s1&code_challenge=c1&code_challenge_method=S256"));
        assert!(url.ends_with("&prompt=select_account"));
    }

    #[test]
    fn profils() {
        let google = Provider::new(ProviderId::Google, "id".into(), "secret".into());
        let profile = google
            .profile(&serde_json::json!({"sub": "123", "name": "Alice", "email": "a@example.org", "email_verified": true}))
            .unwrap();
        assert_eq!(profile.subject, "123");
        assert_eq!(profile.verified_email(), Some("a@example.org"));

        let amazon = Provider::new(ProviderId::Amazon, "id".into(), "secret".into());
        let profile = amazon
            .profile(&serde_json::json!({"user_id": "amzn1.account.X", "name": "Bob", "email": "b@example.org"}))
            .unwrap();
        assert_eq!(profile.subject, "amzn1.account.X");
        assert_eq!(profile.email.as_deref(), Some("b@example.org"));
        assert_eq!(profile.verified_email(), None);

        assert!(
            google
                .profile(&serde_json::json!({"name": "Sans sub"}))
                .is_none()
        );
    }
}
