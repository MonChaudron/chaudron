//! Configuration lue dans l'environnement (fichier `.env` du compose).

use std::env;

use crate::auth::providers::{Provider, ProviderId};

/// Qui peut créer un compte, en plus des invitations (toujours acceptées). Le tout premier compte est libre avec
/// `Closed` ; avec une liste, il doit y figurer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignupPolicy {
    /// Personne (défaut).
    Closed,
    /// Tout le monde (`*`).
    Open,
    /// Ces adresses e-mail, vérifiées par le fournisseur.
    Emails(Vec<String>),
}

impl SignupPolicy {
    /// `SIGNUP_ALLOWED_EMAILS` : vide = fermé, `*` = ouvert, sinon une liste séparée par des virgules.
    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "" => Self::Closed,
            "*" => Self::Open,
            list => Self::Emails(
                list.split(',')
                    .map(|email| email.trim().to_lowercase())
                    .filter(|email| !email.is_empty())
                    .collect(),
            ),
        }
    }

    pub fn allows(&self, verified_email: Option<&str>) -> bool {
        match self {
            Self::Closed => false,
            Self::Open => true,
            Self::Emails(list) => verified_email.is_some_and(|email| {
                list.iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(email))
            }),
        }
    }
}

/// Client autorisé à demander un jeton au nom d'un foyer (la skill Alexa), par le flux du code d'autorisation.
#[derive(Clone, Debug)]
pub struct OAuthClient {
    pub id: String,
    pub secret: String,
    /// Nom affiché sur l'écran d'autorisation et dans « Accès ».
    pub name: String,
    /// Adresses de retour acceptées, comparées à l'identique.
    pub redirect_uris: Vec<String>,
}

pub struct Config {
    /// URL publique, sans `/` final (ex. `https://chaudron.bonvallet.eu`). Sert aux URL de retour OAuth,
    /// aux liens d'invitation, au contrôle de l'en-tête `Origin` et aux cookies `Secure`.
    pub public_url: String,
    /// Fournisseurs configurés, dans l'ordre d'affichage.
    pub providers: Vec<Provider>,
    pub signup: SignupPolicy,
    /// Monte le fournisseur OAuth factice sur `/dev/oauth` (dev seulement, jamais en production).
    pub fake_oauth: bool,
    /// Origine du serveur de dev de l'appli (`http://localhost:8081`) : CORS avec cookies, écritures acceptées,
    /// retour après connexion. Dev seulement.
    pub dev_web_origin: Option<String>,
    /// Clients OAuth : la skill Alexa (`ALEXA_CLIENT_ID`, `ALEXA_CLIENT_SECRET`, `ALEXA_REDIRECT_URIS`).
    pub oauth_clients: Vec<OAuthClient>,
}

impl Config {
    pub fn from_env(port: u16) -> Result<Self, String> {
        let public_url = env::var("PUBLIC_URL")
            .unwrap_or_else(|_| format!("http://localhost:{port}"))
            .trim()
            .trim_end_matches('/')
            .to_owned();
        if !public_url.starts_with("https://") && !public_url.starts_with("http://") {
            return Err("PUBLIC_URL doit commencer par https:// ou http://".into());
        }

        let mut providers = Vec::new();
        for id in [ProviderId::Google, ProviderId::Amazon] {
            let prefix = id.env_prefix();
            match (
                var(&format!("{prefix}_CLIENT_ID")),
                var(&format!("{prefix}_CLIENT_SECRET")),
            ) {
                (Some(client_id), Some(secret)) => {
                    providers.push(Provider::new(id, client_id, secret))
                }
                (None, None) => {}
                _ => {
                    return Err(format!(
                        "{prefix}_CLIENT_ID et {prefix}_CLIENT_SECRET vont ensemble"
                    ));
                }
            }
        }

        let fake_oauth = var("AUTH_FAKE").is_some_and(|v| v == "1" || v == "true");
        if fake_oauth {
            // Le navigateur passe par l'URL publique ; le serveur se parle à lui-même en direct.
            providers.push(Provider::fake(
                &format!("{public_url}/dev/oauth"),
                &format!("http://127.0.0.1:{port}/dev/oauth"),
            ));
        }

        let dev_web_origin = var("DEV_WEB_ORIGIN").map(|o| o.trim_end_matches('/').to_owned());
        if let Some(origin) = &dev_web_origin
            && !origin.starts_with("http://")
            && !origin.starts_with("https://")
        {
            return Err("DEV_WEB_ORIGIN doit commencer par http:// ou https://".into());
        }

        let mut oauth_clients = Vec::new();
        match (
            var("ALEXA_CLIENT_ID"),
            var("ALEXA_CLIENT_SECRET"),
            var("ALEXA_REDIRECT_URIS"),
        ) {
            (Some(id), Some(secret), Some(uris)) => {
                let redirect_uris: Vec<String> = uris
                    .split(',')
                    .map(|u| u.trim().to_owned())
                    .filter(|u| !u.is_empty())
                    .collect();
                if redirect_uris.iter().any(|u| !u.starts_with("https://")) {
                    return Err("ALEXA_REDIRECT_URIS : des adresses https:// seulement".into());
                }
                oauth_clients.push(OAuthClient {
                    id,
                    secret,
                    name: "Alexa".into(),
                    redirect_uris,
                });
            }
            (None, None, None) => {}
            _ => {
                return Err(
                    "ALEXA_CLIENT_ID, ALEXA_CLIENT_SECRET et ALEXA_REDIRECT_URIS vont ensemble"
                        .into(),
                );
            }
        }

        Ok(Self {
            public_url,
            providers,
            signup: SignupPolicy::parse(&env::var("SIGNUP_ALLOWED_EMAILS").unwrap_or_default()),
            fake_oauth,
            dev_web_origin,
            oauth_clients,
        })
    }

    pub fn secure_cookies(&self) -> bool {
        self.public_url.starts_with("https://")
    }

    /// Origine acceptée pour les écritures : l'URL publique, et l'appli de dev si elle est déclarée.
    pub fn allows_origin(&self, origin: &[u8]) -> bool {
        origin == self.public_url.as_bytes()
            || self
                .dev_web_origin
                .as_deref()
                .is_some_and(|o| origin == o.as_bytes())
    }

    /// Adresse d'une page de l'appli : relative, ou sur le serveur de dev de l'appli s'il est déclaré.
    pub fn web_url(&self, path: &str) -> String {
        format!("{}{path}", self.dev_web_origin.as_deref().unwrap_or(""))
    }

    pub fn oauth_client(&self, id: &str) -> Option<&OAuthClient> {
        self.oauth_clients.iter().find(|c| c.id == id)
    }

    pub fn provider(&self, id: &str) -> Option<&Provider> {
        self.providers.iter().find(|p| p.id.as_str() == id)
    }

    pub fn redirect_uri(&self, provider: ProviderId) -> String {
        format!(
            "{}/api/v1/auth/{}/callback",
            self.public_url,
            provider.as_str()
        )
    }
}

fn var(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn politique_d_inscription() {
        assert_eq!(SignupPolicy::parse(""), SignupPolicy::Closed);
        assert_eq!(SignupPolicy::parse(" * "), SignupPolicy::Open);
        let list = SignupPolicy::parse("Alice@Example.org, bob@example.org,");
        assert_eq!(
            list,
            SignupPolicy::Emails(vec!["alice@example.org".into(), "bob@example.org".into()])
        );
        assert!(list.allows(Some("ALICE@example.org")));
        assert!(!list.allows(Some("eve@example.org")));
        assert!(!list.allows(None));
        assert!(!SignupPolicy::Closed.allows(Some("alice@example.org")));
        assert!(SignupPolicy::Open.allows(None));
    }
}
