//! Connexion (OAuth Amazon, Google, factice), sessions web en cookie, compte courant.
//!
//! Session : jeton aléatoire dans un cookie `HttpOnly; SameSite=Lax`, seul son hash est en base ; expiration
//! glissante d'un an. Pas de clé secrète à gérer, et la déconnexion révoque vraiment.

pub mod fake;
pub mod one_tap;
pub mod providers;

use std::convert::Infallible;

use axum::{
    Json, Router,
    extract::{FromRequestParts, OptionalFromRequestParts, Path, Query, Request, State},
    http::{
        HeaderMap, HeaderValue, Method, StatusCode,
        header::{AUTHORIZATION, COOKIE, ORIGIN, SET_COOKIE},
        request::Parts,
    },
    middleware::Next,
    response::{AppendHeaders, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection, PgPool};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    AppState,
    config::SignupPolicy,
    crypto::{constant_time_eq, is_token, pkce_challenge, random_token, sha256},
    error::{ApiError, ApiResult},
    households::{self, HouseholdSummary},
    types::Scope,
};
use providers::{Profile, ProviderId};

/// Durée de vie d'une session, prolongée à chaque usage (au plus une fois par jour).
pub const SESSION_DAYS: i32 = 365;
/// Durée accordée pour aller chez le fournisseur et revenir.
const OAUTH_COOKIE_SECONDS: i64 = 600;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/providers", get(list_providers))
        .route("/auth/signup", get(signup_status))
        .route("/auth/{provider}/login", get(login))
        .route("/auth/{provider}/callback", get(callback))
        .route("/auth/google/one-tap", post(one_tap_login))
        .route("/auth/logout", post(logout))
        .route("/me", get(me).patch(update_me))
}

// Cookies

/// `__Host-` en HTTPS : le cookie ne peut pas être posé par un autre sous-domaine de bonvallet.eu.
pub fn session_cookie_name(secure: bool) -> &'static str {
    if secure {
        "__Host-chaudron_session"
    } else {
        "chaudron_session"
    }
}

fn oauth_cookie_name(secure: bool) -> &'static str {
    if secure {
        "__Host-chaudron_oauth"
    } else {
        "chaudron_oauth"
    }
}

fn cookie(name: &str, value: &str, max_age: i64, secure: bool) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}"
    ))
    .expect("cookie ASCII")
}

fn session_cookie(token: &str, secure: bool) -> HeaderValue {
    cookie(
        session_cookie_name(secure),
        token,
        i64::from(SESSION_DAYS) * 86_400,
        secure,
    )
}

fn read_cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

// Session courante

/// Utilisateur connecté. Extracteur : 401 sans session valide.
#[derive(Clone, Copy, Debug)]
pub struct CurrentUser(pub Uuid);

impl<S: Send + Sync> FromRequestParts<S> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, ApiError> {
        parts
            .extensions
            .get::<CurrentUser>()
            .copied()
            .ok_or_else(ApiError::unauthenticated)
    }
}

impl<S: Send + Sync> OptionalFromRequestParts<S> for CurrentUser {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Option<Self>, Infallible> {
        Ok(parts.extensions.get::<CurrentUser>().copied())
    }
}

/// Jeton d'API reconnu (`Authorization: Bearer chd_…`) : il ne vaut que pour son foyer, et pour ses scopes.
#[derive(Clone, Debug)]
pub struct ApiToken {
    pub household_id: Uuid,
    pub scopes: Vec<Scope>,
    /// Le compte derrière le jeton : celui qui a fait la liaison (Alexa), ou créé le jeton (Claude).
    pub user_id: Option<Uuid>,
    /// Nom signé dans l'historique : celui du jeton (« Claude »), ou du compte pour une liaison.
    pub author_name: String,
}

/// Qui signe une écriture (version, retour, notification).
#[derive(Clone, Debug)]
pub struct Author {
    pub id: Option<Uuid>,
    pub name: String,
}

/// Qui appelle : un compte connecté (toute la cuisine de ses foyers) ou un jeton d'API (son foyer, ses scopes).
/// Extracteur : 401 sans session ni jeton valide.
#[derive(Clone, Debug)]
pub enum Caller {
    User(Uuid),
    Token(ApiToken),
}

impl<S: Send + Sync> FromRequestParts<S> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, ApiError> {
        if let Some(CurrentUser(id)) = parts.extensions.get::<CurrentUser>() {
            return Ok(Self::User(*id));
        }
        parts
            .extensions
            .get::<ApiToken>()
            .cloned()
            .map(Self::Token)
            .ok_or_else(ApiError::unauthenticated)
    }
}

impl Caller {
    /// Accès à un référentiel commun (ingrédients, Ciqual) : tout compte, ou un jeton qui a ce scope.
    pub fn require_scope(&self, scope: Scope) -> ApiResult<()> {
        match self {
            Self::User(_) => Ok(()),
            Self::Token(token) if token.scopes.contains(&scope) => Ok(()),
            Self::Token(_) => Err(ApiError::forbidden(format!(
                "Ce jeton n'a pas le droit « {} ».",
                scope.as_str()
            ))),
        }
    }

    /// Qui signe : le compte connecté, ou le jeton.
    pub async fn author(&self, db: &PgPool) -> ApiResult<Author> {
        Ok(match self {
            Self::User(id) => Author {
                id: Some(*id),
                name: sqlx::query_scalar("SELECT display_name FROM users WHERE id = $1")
                    .bind(id)
                    .fetch_one(db)
                    .await?,
            },
            Self::Token(token) => Author {
                id: token.user_id,
                name: token.author_name.clone(),
            },
        })
    }

    /// Admin de l'instance : un compte connecté seulement (jamais un jeton). 403 sinon.
    pub async fn require_admin(&self, state: &AppState) -> ApiResult<Uuid> {
        match self {
            Self::User(id) if is_admin(state, *id).await? => Ok(*id),
            _ => Err(ApiError::forbidden("Réservé à l'admin de l'instance.")),
        }
    }

    /// Accès au foyer : un compte du foyer, ou un jeton de ce foyer qui a ce scope. 404 hors du foyer.
    pub async fn require(&self, db: &PgPool, household_id: Uuid, scope: Scope) -> ApiResult<()> {
        match self {
            Self::User(user_id) => households::role_in(db, household_id, *user_id)
                .await
                .map(drop),
            Self::Token(token) if token.household_id != household_id => Err(ApiError::not_found()),
            Self::Token(_) => self.require_scope(scope),
        }
    }
}

/// Jeton d'API de l'en-tête `Authorization: Bearer …`, s'il est valide (et note son dernier usage).
async fn bearer_token(db: &PgPool, headers: &HeaderMap) -> Result<Option<ApiToken>, sqlx::Error> {
    let Some(token) = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        return Ok(None);
    };
    let row: Option<(Uuid, Vec<String>, Option<Uuid>, String)> = sqlx::query_as(
        "UPDATE api_tokens t SET last_used_at = now()
         WHERE token_hash = $1 AND (expires_at IS NULL OR expires_at > now())
         RETURNING household_id, scopes, created_by,
                   CASE WHEN grant_id IS NULL THEN name
                        ELSE coalesce((SELECT display_name FROM users u WHERE u.id = t.created_by), name) END",
    )
    .bind(sha256(token.trim().as_bytes()))
    .fetch_optional(db)
    .await?;
    Ok(
        row.map(|(household_id, scopes, user_id, author_name)| ApiToken {
            household_id,
            scopes: scopes
                .into_iter()
                .filter_map(|s| Scope::try_from(s).ok())
                .collect(),
            user_id,
            author_name,
        }),
    )
}

/// Middleware de l'API : lit le cookie de session, place `CurrentUser` dans la requête, prolonge la session.
/// Lit aussi un jeton d'API (`ApiToken`).
pub async fn session_layer(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    match bearer_token(&state.db, request.headers()).await {
        Ok(Some(token)) => {
            request.extensions_mut().insert(token);
        }
        Ok(None) => {}
        Err(err) => return ApiError::from(err).into_response(),
    }
    let secure = state.config.secure_cookies();
    let token = read_cookie(request.headers(), session_cookie_name(secure)).map(str::to_owned);
    let mut renew = None;
    if let Some(token) = token {
        let row: Result<Option<(Uuid, bool)>, _> = sqlx::query_as(
            "SELECT user_id, last_seen_at < now() - interval '1 day'
             FROM sessions WHERE token_hash = $1 AND expires_at > now()",
        )
        .bind(sha256(token.as_bytes()))
        .fetch_optional(&state.db)
        .await;
        match row {
            Ok(Some((user_id, stale))) => {
                request.extensions_mut().insert(CurrentUser(user_id));
                if stale {
                    renew = Some(token);
                }
            }
            Ok(None) => {}
            Err(err) => return ApiError::from(err).into_response(),
        }
    }

    let mut response = next.run(request).await;

    if let Some(token) = renew {
        let renewed = sqlx::query(
            "UPDATE sessions SET last_seen_at = now(), expires_at = now() + make_interval(days => $2)
             WHERE token_hash = $1",
        )
        .bind(sha256(token.as_bytes()))
        .bind(SESSION_DAYS)
        .execute(&state.db)
        .await;
        // Pas de nouveau cookie si la réponse en pose déjà un (déconnexion, nouvelle connexion).
        if renewed.is_ok() && !response.headers().contains_key(SET_COOKIE) {
            response
                .headers_mut()
                .append(SET_COOKIE, session_cookie(&token, secure));
        }
    }
    response
}

/// Middleware de l'API : refuse les écritures venues d'un autre site (en plus de `SameSite=Lax`).
pub async fn origin_guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let safe = matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    if !safe
        && let Some(origin) = request.headers().get(ORIGIN)
        && !state.config.allows_origin(origin.as_bytes())
    {
        return ApiError::forbidden("Origine refusée.").into_response();
    }
    next.run(request).await
}

async fn create_session(db: &PgPool, user_id: Uuid) -> Result<String, sqlx::Error> {
    sqlx::query("DELETE FROM sessions WHERE expires_at < now()")
        .execute(db)
        .await?;
    let token = random_token();
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, expires_at)
         VALUES ($1, $2, now() + make_interval(days => $3))",
    )
    .bind(sha256(token.as_bytes()))
    .bind(user_id)
    .bind(SESSION_DAYS)
    .execute(db)
    .await?;
    Ok(token)
}

// Connexion

/// Fournisseur de connexion proposé par l'instance.
#[derive(Serialize, ToSchema)]
pub struct ProviderInfo {
    pub id: ProviderId,
    /// Identifiant client public (Google seulement) : sert à Google One Tap.
    pub client_id: Option<String>,
}

/// Fournisseurs de connexion disponibles sur cette instance, dans l'ordre d'affichage.
#[utoipa::path(
    get,
    path = "/api/v1/auth/providers",
    tag = "connexion",
    security(()),
    responses((status = 200, body = Vec<ProviderInfo>))
)]
async fn list_providers(State(state): State<AppState>) -> Json<Vec<ProviderInfo>> {
    Json(
        state
            .config
            .providers
            .iter()
            .map(|p| ProviderInfo {
                id: p.id,
                client_id: (p.id == ProviderId::Google).then(|| p.client_id.clone()),
            })
            .collect(),
    )
}

/// Qui peut créer un compte sur l'instance (les invitations marchent toujours).
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Signup {
    /// Aucun compte encore : le premier est libre (il crée l'instance).
    First,
    /// Tout le monde (`SIGNUP_ALLOWED_EMAILS=*`).
    Open,
    /// Quelques adresses seulement : les autres entrent par invitation.
    Allowlist,
    /// Sur invitation seulement.
    Invite,
}

#[derive(Serialize, ToSchema)]
pub struct SignupInfo {
    pub signup: Signup,
}

/// Qui peut créer un compte : la page d'accueil propose « créer un foyer » ou seulement « j'ai une invitation ».
#[utoipa::path(
    get,
    path = "/api/v1/auth/signup",
    tag = "connexion",
    security(()),
    responses((status = 200, body = SignupInfo))
)]
async fn signup_status(State(state): State<AppState>) -> ApiResult<Json<SignupInfo>> {
    let signup = match &state.config.signup {
        SignupPolicy::Open => Signup::Open,
        SignupPolicy::Emails(_) => Signup::Allowlist,
        SignupPolicy::Closed => {
            let first: bool = sqlx::query_scalar("SELECT NOT EXISTS (SELECT 1 FROM users)")
                .fetch_one(&state.db)
                .await?;
            if first { Signup::First } else { Signup::Invite }
        }
    };
    Ok(Json(SignupInfo { signup }))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct LoginQuery {
    /// Jeton d'un lien d'invitation : permet de créer un compte et rejoint le foyer.
    invite: Option<String>,
    /// Chemin de l'appli où revenir après la connexion (par défaut `/`, ou la page de l'invitation).
    return_to: Option<String>,
}

/// Ce qu'on retient le temps de l'aller-retour chez le fournisseur (cookie `HttpOnly`, 10 min).
#[derive(Serialize, Deserialize)]
struct Pending {
    provider: String,
    state: String,
    verifier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    invite: Option<String>,
    return_to: String,
}

/// Un chemin local seulement : pas de redirection ouverte vers un autre site.
fn safe_return_to(value: Option<&str>) -> Option<String> {
    value
        .filter(|v| {
            v.starts_with('/')
                && !v.starts_with("//")
                && !v.starts_with("/\\")
                && v.len() <= 512
                && v.bytes().all(|b| b.is_ascii_graphic())
        })
        .map(str::to_owned)
}

/// Démarre la connexion : redirige vers le fournisseur. Connecté, lie ce fournisseur au compte courant.
#[utoipa::path(
    get,
    path = "/api/v1/auth/{provider}/login",
    tag = "connexion",
    security(()),
    params(("provider" = ProviderId, Path), LoginQuery),
    responses(
        (status = 303, description = "Vers la page de connexion du fournisseur"),
        (status = 404, description = "Fournisseur non configuré", body = crate::error::ErrorBody),
    )
)]
async fn login(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(query): Query<LoginQuery>,
) -> ApiResult<Response> {
    let provider = state
        .config
        .provider(&provider)
        .ok_or_else(ApiError::not_found)?;
    let invite = query.invite.filter(|t| is_token(t));
    let return_to = safe_return_to(query.return_to.as_deref())
        .or_else(|| invite.as_ref().map(|t| format!("/invitation/{t}")))
        .unwrap_or_else(|| "/".into());
    let pending = Pending {
        provider: provider.id.as_str().into(),
        state: random_token(),
        verifier: random_token(),
        invite,
        return_to,
    };
    let url = provider.authorization_url(
        &state.config.redirect_uri(provider.id),
        &pending.state,
        &pkce_challenge(&pending.verifier),
    );
    let secure = state.config.secure_cookies();
    let value = serde_urlencoded::to_string(&pending).map_err(|_| ApiError::internal())?;
    Ok((
        AppendHeaders([(
            SET_COOKIE,
            cookie(
                oauth_cookie_name(secure),
                &value,
                OAUTH_COOKIE_SECONDS,
                secure,
            ),
        )]),
        Redirect::to(&url),
    )
        .into_response())
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    /// Posé par le fournisseur en cas de refus (`access_denied`) ou d'erreur.
    error: Option<String>,
}

/// Retour du fournisseur : crée ou retrouve le compte, ouvre la session, revient dans l'appli.
///
/// En cas d'échec, redirige vers `/connexion?erreur=…` : `refus`, `fournisseur`, `expiree`,
/// `inscriptions-fermees`, `deja-lie`, `interne`.
#[utoipa::path(
    get,
    path = "/api/v1/auth/{provider}/callback",
    tag = "connexion",
    security(()),
    params(("provider" = ProviderId, Path), CallbackQuery),
    responses((status = 303, description = "Vers l'appli, connecté, ou vers /connexion?erreur=…"))
)]
async fn callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(query): Query<CallbackQuery>,
    current: Option<CurrentUser>,
    headers: HeaderMap,
) -> Response {
    let secure = state.config.secure_cookies();
    let clear = (SET_COOKIE, cookie(oauth_cookie_name(secure), "", 0, secure));
    match finish_login(&state, &provider, query, current, &headers).await {
        Ok((token, return_to)) => (
            AppendHeaders([clear, (SET_COOKIE, session_cookie(&token, secure))]),
            Redirect::to(&state.config.web_url(&return_to)),
        )
            .into_response(),
        Err(code) => (
            AppendHeaders([clear]),
            Redirect::to(&state.config.web_url(&format!("/connexion?erreur={code}"))),
        )
            .into_response(),
    }
}

async fn finish_login(
    state: &AppState,
    provider: &str,
    query: CallbackQuery,
    current: Option<CurrentUser>,
    headers: &HeaderMap,
) -> Result<(String, String), &'static str> {
    if let Some(error) = query.error {
        return Err(if error == "access_denied" {
            "refus"
        } else {
            "fournisseur"
        });
    }
    let provider = state.config.provider(provider).ok_or("fournisseur")?;
    let pending: Pending = read_cookie(headers, oauth_cookie_name(state.config.secure_cookies()))
        .and_then(|v| serde_urlencoded::from_str(v).ok())
        .ok_or("expiree")?;
    let state_ok = query
        .state
        .is_some_and(|s| constant_time_eq(s.as_bytes(), pending.state.as_bytes()));
    if pending.provider != provider.id.as_str() || !state_ok {
        return Err("expiree");
    }
    let code = query.code.ok_or("fournisseur")?;

    let profile = provider
        .fetch_profile(
            &state.http,
            &code,
            &state.config.redirect_uri(provider.id),
            &pending.verifier,
        )
        .await
        .map_err(|err| {
            tracing::warn!(provider = provider.id.as_str(), %err, "connexion refusée par le fournisseur");
            "fournisseur"
        })?;

    let user_id = resolve_user(
        state,
        provider.id,
        &profile,
        current,
        pending.invite.as_deref(),
    )
    .await?;
    let token = create_session(&state.db, user_id).await.map_err(|err| {
        tracing::error!(%err, "création de session");
        "interne"
    })?;
    Ok((token, pending.return_to))
}

/// Retrouve le compte de cette identité, ou la lie au compte connecté, ou crée un compte si c'est permis :
/// premier compte de l'instance (sans liste d'adresses), invitation valide, ou adresse autorisée par
/// `SIGNUP_ALLOWED_EMAILS`.
async fn resolve_user(
    state: &AppState,
    provider: ProviderId,
    profile: &Profile,
    current: Option<CurrentUser>,
    invite: Option<&str>,
) -> Result<Uuid, &'static str> {
    let internal = |err: sqlx::Error| {
        tracing::error!(%err, "connexion");
        "interne"
    };
    let mut tx = state.db.begin().await.map_err(internal)?;
    // Sérialise les connexions : deux « premiers comptes » simultanés ne passent pas tous les deux.
    sqlx::query("SELECT pg_advisory_xact_lock(4231700)")
        .execute(&mut *tx)
        .await
        .map_err(internal)?;

    let existing: Option<Uuid> =
        sqlx::query_scalar("SELECT user_id FROM identities WHERE provider = $1 AND subject = $2")
            .bind(provider.as_str())
            .bind(&profile.subject)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal)?;

    let user_id = match (existing, current) {
        (Some(user_id), Some(CurrentUser(current))) if user_id != current => {
            return Err("deja-lie");
        }
        (Some(user_id), _) => {
            sqlx::query(
                "UPDATE identities SET email = $3, email_verified = $4, updated_at = now()
                 WHERE provider = $1 AND subject = $2",
            )
            .bind(provider.as_str())
            .bind(&profile.subject)
            .bind(&profile.email)
            .bind(profile.email_verified)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
            user_id
        }
        (None, Some(CurrentUser(current))) => {
            insert_identity(&mut tx, current, provider, profile)
                .await
                .map_err(internal)?;
            tracing::info!(user = %current, provider = provider.as_str(), "fournisseur lié au compte");
            current
        }
        (None, None) => {
            let first: bool = sqlx::query_scalar("SELECT NOT EXISTS (SELECT 1 FROM users)")
                .fetch_one(&mut *tx)
                .await
                .map_err(internal)?;
            let invited = match invite {
                Some(token) => households::pending_invite(&mut tx, token)
                    .await
                    .map_err(internal)?
                    .is_some(),
                None => false,
            };
            // Le premier compte est libre, sauf si une liste d'adresses est définie : elle s'applique
            // alors aussi à lui (personne ne devance le propriétaire sur une instance exposée).
            let bootstrap = first && state.config.signup == SignupPolicy::Closed;
            if !(bootstrap || invited || state.config.signup.allows(profile.verified_email())) {
                return Err("inscriptions-fermees");
            }
            let user_id = Uuid::now_v7();
            let display_name = profile
                .name
                .clone()
                .or_else(|| {
                    profile
                        .email
                        .as_deref()
                        .and_then(|e| e.split('@').next())
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| "Sans nom".into());
            sqlx::query("INSERT INTO users (id, display_name, email) VALUES ($1, $2, $3)")
                .bind(user_id)
                .bind(&display_name)
                .bind(&profile.email)
                .execute(&mut *tx)
                .await
                .map_err(internal)?;
            insert_identity(&mut tx, user_id, provider, profile)
                .await
                .map_err(internal)?;
            tracing::info!(user = %user_id, provider = provider.as_str(), first, invited, "compte créé");
            user_id
        }
    };

    if let Some(token) = invite {
        // Invitation expirée ou déjà servie : on connecte quand même, la page de l'invitation le dira.
        households::accept_invite(&mut tx, token, user_id)
            .await
            .map_err(internal)?;
    }
    tx.commit().await.map_err(internal)?;
    Ok(user_id)
}

async fn insert_identity(
    conn: &mut PgConnection,
    user_id: Uuid,
    provider: ProviderId,
    profile: &Profile,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO identities (provider, subject, user_id, email, email_verified) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(provider.as_str())
    .bind(&profile.subject)
    .bind(user_id)
    .bind(&profile.email)
    .bind(profile.email_verified)
    .execute(conn)
    .await?;
    Ok(())
}

/// Jeton reçu de Google One Tap.
#[derive(Deserialize, ToSchema)]
pub struct OneTapLogin {
    /// Jeton d'identité (JWT) donné par Google à la page.
    pub credential: String,
    /// Jeton d'un lien d'invitation, si la connexion se fait depuis l'invitation.
    pub invite: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct LoggedIn {
    /// Page de l'appli où aller ensuite.
    pub return_to: String,
}

/// Erreurs de connexion vues par l'API (One Tap), à partir des codes de la connexion classique.
fn login_error(code: &'static str) -> ApiError {
    match code {
        "inscriptions-fermees" => ApiError::new(
            StatusCode::FORBIDDEN,
            "signup_closed",
            "Les inscriptions sont fermées : il faut un lien d’invitation.",
        ),
        "deja-lie" => ApiError::conflict(
            "already_linked",
            "Ce compte Google est déjà lié à un autre compte Chaudron.",
        ),
        _ => ApiError::internal(),
    }
}

/// Connexion par Google One Tap : vérifie le jeton de Google, puis comme la connexion classique (compte,
/// invitation, session).
#[utoipa::path(
    post,
    path = "/api/v1/auth/google/one-tap",
    tag = "connexion",
    security(()),
    request_body = OneTapLogin,
    responses(
        (status = 200, description = "Connecté (cookie de session posé)", body = LoggedIn),
        (status = 401, description = "Jeton refusé", body = crate::error::ErrorBody),
        (status = 403, description = "Inscriptions fermées", body = crate::error::ErrorBody),
        (status = 404, description = "Google non configuré", body = crate::error::ErrorBody),
        (status = 409, description = "Compte Google lié à un autre compte", body = crate::error::ErrorBody),
    )
)]
async fn one_tap_login(
    State(state): State<AppState>,
    current: Option<CurrentUser>,
    Json(body): Json<OneTapLogin>,
) -> ApiResult<Response> {
    let google = state
        .config
        .provider(ProviderId::Google.as_str())
        .ok_or_else(ApiError::not_found)?;
    let profile = one_tap::verify(&state.google_keys, &state.http, google, &body.credential)
        .await
        .map_err(|err| {
            tracing::warn!(%err, "jeton Google One Tap refusé");
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "one_tap_refused",
                "Connexion Google refusée. Réessaie.",
            )
        })?;
    let invite = body.invite.filter(|t| is_token(t));
    let user_id = resolve_user(
        &state,
        ProviderId::Google,
        &profile,
        current,
        invite.as_deref(),
    )
    .await
    .map_err(login_error)?;
    let token = create_session(&state.db, user_id).await?;
    let return_to = invite.map_or_else(|| "/".into(), |t| format!("/invitation/{t}"));
    Ok((
        AppendHeaders([(
            SET_COOKIE,
            session_cookie(&token, state.config.secure_cookies()),
        )]),
        Json(LoggedIn { return_to }),
    )
        .into_response())
}

/// Ferme la session courante (sur cet appareil).
#[utoipa::path(
    post,
    path = "/api/v1/auth/logout",
    tag = "connexion",
    responses((status = 204, description = "Déconnecté"))
)]
async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let secure = state.config.secure_cookies();
    if let Some(token) = read_cookie(&headers, session_cookie_name(secure)) {
        sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
            .bind(sha256(token.as_bytes()))
            .execute(&state.db)
            .await?;
    }
    Ok((
        StatusCode::NO_CONTENT,
        AppendHeaders([(
            SET_COOKIE,
            cookie(session_cookie_name(secure), "", 0, secure),
        )]),
    )
        .into_response())
}

// Compte courant

/// Utilisateur connecté, ses fournisseurs liés et ses foyers.
#[derive(Serialize, ToSchema)]
pub struct Me {
    pub id: Uuid,
    pub display_name: String,
    pub email: Option<String>,
    pub identities: Vec<Identity>,
    pub households: Vec<HouseholdSummary>,
    /// Admin de l'instance (`ADMIN_EMAILS`, adresse vérifiée) : « À revoir ».
    pub instance_admin: bool,
    /// Notifications pas encore lues (le badge).
    pub unread_notifications: i64,
}

#[derive(Serialize, ToSchema, FromRow)]
pub struct Identity {
    #[schema(value_type = ProviderId)]
    pub provider: String,
    pub email: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/v1/me",
    tag = "compte",
    responses(
        (status = 200, body = Me),
        (status = 401, body = crate::error::ErrorBody),
    )
)]
async fn me(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
) -> ApiResult<Json<Me>> {
    load_me(&state, user_id).await.map(Json)
}

/// Admin de l'instance : une adresse de `ADMIN_EMAILS`, vérifiée par le fournisseur.
pub async fn is_admin(state: &AppState, user_id: Uuid) -> ApiResult<bool> {
    if state.config.admin_emails.is_empty() {
        return Ok(false);
    }
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM identities
                        WHERE user_id = $1 AND email_verified AND lower(email) = ANY($2))",
    )
    .bind(user_id)
    .bind(&state.config.admin_emails)
    .fetch_one(&state.db)
    .await?)
}

async fn load_me(state: &AppState, user_id: Uuid) -> ApiResult<Me> {
    let db = &state.db;
    let (display_name, email): (String, Option<String>) =
        sqlx::query_as("SELECT display_name, email FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(db)
            .await?;
    let identities = sqlx::query_as(
        "SELECT provider, email FROM identities WHERE user_id = $1 ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;
    let households = households::summaries(db, user_id).await?;
    Ok(Me {
        id: user_id,
        display_name,
        email,
        identities,
        households,
        instance_admin: is_admin(state, user_id).await?,
        unread_notifications: crate::notifications::unread(&mut *db.acquire().await?, user_id)
            .await?,
    })
}

#[derive(Deserialize, ToSchema)]
pub struct MeUpdate {
    /// Nom affiché aux autres comptes du foyer.
    pub display_name: String,
}

#[utoipa::path(
    patch,
    path = "/api/v1/me",
    tag = "compte",
    request_body = MeUpdate,
    responses(
        (status = 200, body = Me),
        (status = 422, body = crate::error::ErrorBody),
    )
)]
async fn update_me(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Json(update): Json<MeUpdate>,
) -> ApiResult<Json<Me>> {
    let name = crate::validate::name(&update.display_name)?;
    sqlx::query("UPDATE users SET display_name = $2, updated_at = now() WHERE id = $1")
        .bind(user_id)
        .bind(name)
        .execute(&state.db)
        .await?;
    load_me(&state, user_id).await.map(Json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retour_local_seulement() {
        assert_eq!(safe_return_to(Some("/foyer")).as_deref(), Some("/foyer"));
        assert_eq!(safe_return_to(Some("//evil.example")), None);
        assert_eq!(safe_return_to(Some("/\\evil.example")), None);
        assert_eq!(safe_return_to(Some("https://evil.example")), None);
        assert_eq!(safe_return_to(Some("/a b")), None);
        assert_eq!(safe_return_to(None), None);
    }

    #[test]
    fn lecture_des_cookies() {
        let mut headers = HeaderMap::new();
        headers.append(
            COOKIE,
            HeaderValue::from_static("a=1; chaudron_session=abc"),
        );
        headers.append(COOKIE, HeaderValue::from_static("b=2"));
        assert_eq!(read_cookie(&headers, "chaudron_session"), Some("abc"));
        assert_eq!(read_cookie(&headers, "b"), Some("2"));
        assert_eq!(read_cookie(&headers, "c"), None);
    }

    #[test]
    fn cookie_securise_en_https() {
        let value = session_cookie("t", true);
        assert_eq!(
            value.to_str().unwrap(),
            "__Host-chaudron_session=t; Path=/; HttpOnly; SameSite=Lax; Max-Age=31536000; Secure"
        );
    }
}
