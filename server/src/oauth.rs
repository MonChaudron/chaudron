//! Liaison de compte : Chaudron serveur OAuth2 minimal pour la skill Alexa (code d'autorisation + renouvellement,
//! RFC 6749). L'écran « Autoriser Alexa » de l'appli présente le client (`/oauth/client`), puis un compte du foyer
//! autorise (`/oauth/authorize`) ; Amazon échange le code puis renouvelle (`/oauth/token`). Les jetons d'accès
//! durent une heure et vivent dans `api_tokens` : la vérification du `Bearer` est la même que pour Claude.

use axum::{
    Form, Json, Router,
    extract::{Path, Query, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{AUTHORIZATION, CACHE_CONTROL, PRAGMA, WWW_AUTHENTICATE},
    },
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{Caller, CurrentUser},
    config::{Config, OAuthClient},
    crypto::{constant_time_eq, random_token, sha256},
    error::{ApiError, ApiResult, ErrorBody},
    households::{require_owner, role_in},
    types::Scope,
};

/// Durée d'un jeton d'accès.
const ACCESS_SECONDS: i64 = 3600;
/// Durée d'un code d'autorisation.
const CODE_MINUTES: i32 = 5;
/// Ce que la skill peut faire : lire, et avancer dans la cuisson.
const SCOPES: [Scope; 2] = [Scope::Read, Scope::CookingWrite];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/oauth/client", get(client_info))
        .route("/oauth/authorize", post(authorize))
        .route("/oauth/token", post(token))
        .route("/token", get(token_info))
        .route("/households/{id}/links", get(list_links))
        .route("/households/{id}/links/{link_id}", delete(revoke_link))
}

/// Le client, s'il existe et si l'adresse de retour est la sienne (à l'identique).
fn client_for<'a>(config: &'a Config, id: &str, redirect_uri: &str) -> ApiResult<&'a OAuthClient> {
    config
        .oauth_client(id)
        .filter(|c| c.redirect_uris.iter().any(|u| u == redirect_uri))
        .ok_or_else(|| ApiError::invalid("Application ou adresse de retour inconnue."))
}

// Écran d'autorisation

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ClientQuery {
    pub client_id: String,
    pub redirect_uri: String,
}

/// Application qui demande l'accès.
#[derive(Serialize, ToSchema)]
pub struct ClientInfo {
    /// « Alexa ».
    pub name: String,
    /// Ce qu'elle pourra faire.
    pub scopes: Vec<Scope>,
}

/// Présente l'application qui demande l'accès (écran « Autoriser Alexa »).
#[utoipa::path(
    get,
    path = "/api/v1/oauth/client",
    tag = "liaison",
    security(()),
    params(ClientQuery),
    responses(
        (status = 200, body = ClientInfo),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn client_info(
    State(state): State<AppState>,
    Query(query): Query<ClientQuery>,
) -> ApiResult<Json<ClientInfo>> {
    let client = client_for(&state.config, &query.client_id, &query.redirect_uri)?;
    Ok(Json(ClientInfo {
        name: client.name.clone(),
        scopes: SCOPES.to_vec(),
    }))
}

/// « Autoriser » : le foyer choisi, et ce que l'application a envoyé.
#[derive(Deserialize, ToSchema)]
pub struct AuthorizeInput {
    pub client_id: String,
    pub redirect_uri: String,
    /// Rendu tel quel à l'application.
    pub state: Option<String>,
    pub household_id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct Authorized {
    /// Où envoyer le navigateur : l'adresse de retour, avec le code et `state`.
    pub redirect_to: String,
}

/// Autorise l'application sur un foyer du compte connecté (tout compte du foyer) : crée un code de 5 minutes.
#[utoipa::path(
    post,
    path = "/api/v1/oauth/authorize",
    tag = "liaison",
    request_body = AuthorizeInput,
    responses(
        (status = 200, body = Authorized),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn authorize(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Json(body): Json<AuthorizeInput>,
) -> ApiResult<Json<Authorized>> {
    let client = client_for(&state.config, &body.client_id, &body.redirect_uri)?;
    role_in(&state.db, body.household_id, user_id).await?;
    sqlx::query("DELETE FROM oauth_grants WHERE refresh_hash IS NULL AND code_expires_at < now()")
        .execute(&state.db)
        .await?;
    let code = random_token();
    let scopes: Vec<&str> = SCOPES.iter().map(|s| s.as_str()).collect();
    sqlx::query(
        "INSERT INTO oauth_grants (id, household_id, user_id, client_id, scopes, redirect_uri, code_hash, code_expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, now() + make_interval(mins => $8))",
    )
    .bind(Uuid::now_v7())
    .bind(body.household_id)
    .bind(user_id)
    .bind(&client.id)
    .bind(&scopes)
    .bind(&body.redirect_uri)
    .bind(sha256(code.as_bytes()))
    .bind(CODE_MINUTES)
    .execute(&state.db)
    .await?;
    let mut params = vec![("code", code.as_str())];
    if let Some(s) = body.state.as_deref() {
        params.push(("state", s));
    }
    let query = serde_urlencoded::to_string(params).map_err(|_| ApiError::invalid("state"))?;
    let separator = if body.redirect_uri.contains('?') {
        '&'
    } else {
        '?'
    };
    Ok(Json(Authorized {
        redirect_to: format!("{}{separator}{query}", body.redirect_uri),
    }))
}

// Échange de jetons (Amazon, de serveur à serveur)

/// Requête de jeton (RFC 6749 §4.1.3 et §6), en `application/x-www-form-urlencoded`. Le client s'identifie en HTTP
/// Basic, ou par `client_id` et `client_secret`.
#[derive(Deserialize, ToSchema)]
pub struct TokenRequest {
    /// `authorization_code` ou `refresh_token`.
    pub grant_type: String,
    pub code: Option<String>,
    pub redirect_uri: Option<String>,
    pub refresh_token: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct TokenResponse {
    pub access_token: String,
    /// `Bearer`.
    pub token_type: &'static str,
    /// En secondes.
    pub expires_in: i64,
    /// Seulement à l'échange du code : il ne change pas ensuite.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    pub scope: String,
}

/// Erreur au format OAuth2 : `invalid_request`, `invalid_client`, `invalid_grant`, `unsupported_grant_type`.
#[derive(Serialize, ToSchema)]
pub struct OAuthError {
    pub error: &'static str,
    pub error_description: &'static str,
}

fn oauth_error(error: &'static str, error_description: &'static str) -> Response {
    let status = if error == "invalid_client" {
        StatusCode::UNAUTHORIZED
    } else {
        StatusCode::BAD_REQUEST
    };
    let mut response = (
        status,
        Json(OAuthError {
            error,
            error_description,
        }),
    )
        .into_response();
    if status == StatusCode::UNAUTHORIZED {
        response
            .headers_mut()
            .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Basic"));
    }
    no_store(response)
}

fn no_store(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(PRAGMA, HeaderValue::from_static("no-cache"));
    response
}

/// Identifiants du client : en-tête `Authorization: Basic`, sinon le corps.
fn client_credentials(headers: &HeaderMap, body: &TokenRequest) -> Option<(String, String)> {
    let basic = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|v| STANDARD.decode(v.trim()).ok())
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|v| {
            v.split_once(':')
                .map(|(id, secret)| (id.to_owned(), secret.to_owned()))
        });
    basic.or_else(|| Some((body.client_id.clone()?, body.client_secret.clone()?)))
}

#[derive(FromRow)]
struct GrantRow {
    id: Uuid,
    household_id: Uuid,
    user_id: Uuid,
    scopes: Vec<String>,
}

/// Échange un code contre des jetons, ou renouvelle le jeton d'accès.
#[utoipa::path(
    post,
    path = "/api/v1/oauth/token",
    tag = "liaison",
    security(()),
    request_body(content = TokenRequest, content_type = "application/x-www-form-urlencoded"),
    responses(
        (status = 200, body = TokenResponse),
        (status = 400, body = OAuthError),
        (status = 401, body = OAuthError),
    )
)]
pub async fn token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(body): Form<TokenRequest>,
) -> ApiResult<Response> {
    let Some((client_id, secret)) = client_credentials(&headers, &body) else {
        return Ok(oauth_error(
            "invalid_client",
            "Identifiants du client absents.",
        ));
    };
    let Some(client) = state
        .config
        .oauth_client(&client_id)
        .filter(|c| constant_time_eq(c.secret.as_bytes(), secret.as_bytes()))
    else {
        return Ok(oauth_error("invalid_client", "Client inconnu."));
    };
    let (grant, refresh) = match body.grant_type.as_str() {
        "authorization_code" => {
            let (Some(code), Some(redirect_uri)) = (&body.code, &body.redirect_uri) else {
                return Ok(oauth_error(
                    "invalid_request",
                    "code et redirect_uri attendus.",
                ));
            };
            let refresh = random_token();
            let grant: Option<GrantRow> = sqlx::query_as(
                "UPDATE oauth_grants SET code_hash = NULL, code_expires_at = NULL, refresh_hash = $4,
                        last_used_at = now()
                 WHERE code_hash = $1 AND client_id = $2 AND redirect_uri = $3 AND code_expires_at > now()
                 RETURNING id, household_id, user_id, scopes",
            )
            .bind(sha256(code.as_bytes()))
            .bind(&client.id)
            .bind(redirect_uri)
            .bind(sha256(refresh.as_bytes()))
            .fetch_optional(&state.db)
            .await?;
            let Some(grant) = grant else {
                return Ok(oauth_error(
                    "invalid_grant",
                    "Code inconnu, expiré ou déjà utilisé.",
                ));
            };
            // Relier de nouveau remplace la liaison précédente de ce compte.
            sqlx::query(
                "DELETE FROM oauth_grants
                 WHERE household_id = $1 AND user_id = $2 AND client_id = $3 AND id <> $4 AND refresh_hash IS NOT NULL",
            )
            .bind(grant.household_id)
            .bind(grant.user_id)
            .bind(&client.id)
            .bind(grant.id)
            .execute(&state.db)
            .await?;
            (grant, Some(refresh))
        }
        "refresh_token" => {
            let Some(refresh) = &body.refresh_token else {
                return Ok(oauth_error("invalid_request", "refresh_token attendu."));
            };
            let grant: Option<GrantRow> = sqlx::query_as(
                "UPDATE oauth_grants SET last_used_at = now()
                 WHERE refresh_hash = $1 AND client_id = $2
                 RETURNING id, household_id, user_id, scopes",
            )
            .bind(sha256(refresh.as_bytes()))
            .bind(&client.id)
            .fetch_optional(&state.db)
            .await?;
            let Some(grant) = grant else {
                return Ok(oauth_error("invalid_grant", "Liaison inconnue ou retirée."));
            };
            (grant, None)
        }
        _ => {
            return Ok(oauth_error(
                "unsupported_grant_type",
                "authorization_code ou refresh_token.",
            ));
        }
    };

    sqlx::query("DELETE FROM api_tokens WHERE grant_id = $1 AND expires_at <= now()")
        .bind(grant.id)
        .execute(&state.db)
        .await?;
    let access = format!("chd_{}", random_token());
    sqlx::query(
        "INSERT INTO api_tokens (id, household_id, name, token_hash, scopes, created_by, expires_at, grant_id)
         VALUES ($1, $2, $3, $4, $5, $6, now() + make_interval(secs => $7), $8)",
    )
    .bind(Uuid::now_v7())
    .bind(grant.household_id)
    .bind(&client.name)
    .bind(sha256(access.as_bytes()))
    .bind(&grant.scopes)
    .bind(grant.user_id)
    .bind(ACCESS_SECONDS as f64)
    .bind(grant.id)
    .execute(&state.db)
    .await?;
    Ok(no_store(
        Json(TokenResponse {
            access_token: access,
            token_type: "Bearer",
            expires_in: ACCESS_SECONDS,
            refresh_token: refresh,
            scope: grant.scopes.join(" "),
        })
        .into_response(),
    ))
}

// Le jeton appelant

/// Ce que vaut le jeton appelant.
#[derive(Serialize, ToSchema)]
pub struct TokenInfo {
    pub household_id: Uuid,
    pub scopes: Vec<Scope>,
}

/// Foyer et droits du jeton appelant : la skill ne reçoit d'Amazon que le jeton. Jeton seulement (401 en session).
#[utoipa::path(
    get,
    path = "/api/v1/token",
    tag = "liaison",
    responses(
        (status = 200, body = TokenInfo),
        (status = 401, body = ErrorBody),
    )
)]
pub async fn token_info(caller: Caller) -> ApiResult<Json<TokenInfo>> {
    match caller {
        Caller::Token(token) => Ok(Json(TokenInfo {
            household_id: token.household_id,
            scopes: token.scopes,
        })),
        Caller::User(_) => Err(ApiError::unauthenticated()),
    }
}

// Les liaisons du foyer (écran « Accès »)

/// Application liée au foyer.
#[derive(Serialize, ToSchema)]
pub struct LinkInfo {
    pub id: Uuid,
    /// « Alexa ».
    pub client: String,
    /// Le compte qui l'a autorisée.
    pub user: String,
    pub scopes: Vec<Scope>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_used_at: Option<OffsetDateTime>,
}

#[derive(FromRow)]
struct LinkRow {
    id: Uuid,
    client_id: String,
    user_name: String,
    scopes: Vec<String>,
    created_at: OffsetDateTime,
    last_used_at: Option<OffsetDateTime>,
}

/// Applications liées au foyer (propriétaires).
#[utoipa::path(
    get,
    path = "/api/v1/households/{id}/links",
    tag = "liaison",
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, body = Vec<LinkInfo>),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn list_links(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<LinkInfo>>> {
    require_owner(&state.db, id, user_id).await?;
    let rows: Vec<LinkRow> = sqlx::query_as(
        "SELECT g.id, g.client_id, u.display_name AS user_name, g.scopes, g.created_at,
                greatest(g.last_used_at, (SELECT max(t.last_used_at) FROM api_tokens t WHERE t.grant_id = g.id))
                    AS last_used_at
         FROM oauth_grants g JOIN users u ON u.id = g.user_id
         WHERE g.household_id = $1 AND g.refresh_hash IS NOT NULL ORDER BY g.created_at",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| LinkInfo {
                id: row.id,
                client: state
                    .config
                    .oauth_client(&row.client_id)
                    .map_or(row.client_id, |c| c.name.clone()),
                user: row.user_name,
                scopes: row
                    .scopes
                    .into_iter()
                    .filter_map(|s| Scope::try_from(s).ok())
                    .collect(),
                created_at: row.created_at,
                last_used_at: row.last_used_at,
            })
            .collect(),
    ))
}

/// Retire une liaison et ses jetons (propriétaires). L'application devra être liée de nouveau.
#[utoipa::path(
    delete,
    path = "/api/v1/households/{id}/links/{link_id}",
    tag = "liaison",
    params(("id" = Uuid, Path), ("link_id" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn revoke_link(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Path((id, link_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    require_owner(&state.db, id, user_id).await?;
    let deleted = sqlx::query("DELETE FROM oauth_grants WHERE id = $1 AND household_id = $2")
        .bind(link_id)
        .bind(id)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}
