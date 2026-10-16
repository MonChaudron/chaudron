//! Jetons d'API du foyer, pour Claude et les scripts : gérés par les propriétaires, montrés une seule fois.

use std::collections::BTreeSet;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get},
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::CurrentUser,
    crypto::{random_token, sha256},
    error::{ApiError, ApiResult, ErrorBody},
    households::require_owner,
    types::Scope,
    validate,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/households/{id}/tokens",
            get(list_tokens).post(create_token),
        )
        .route("/households/{id}/tokens/{token_id}", delete(revoke_token))
}

/// Jeton d'API, sans sa valeur.
#[derive(Serialize, ToSchema)]
pub struct ApiTokenInfo {
    pub id: Uuid,
    pub name: String,
    pub scopes: Vec<Scope>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_used_at: Option<OffsetDateTime>,
}

#[derive(FromRow)]
struct TokenRow {
    id: Uuid,
    name: String,
    scopes: Vec<String>,
    created_at: OffsetDateTime,
    last_used_at: Option<OffsetDateTime>,
}

impl From<TokenRow> for ApiTokenInfo {
    fn from(row: TokenRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            scopes: row
                .scopes
                .into_iter()
                .filter_map(|s| Scope::try_from(s).ok())
                .collect(),
            created_at: row.created_at,
            last_used_at: row.last_used_at,
        }
    }
}

/// Jetons du foyer (propriétaires).
#[utoipa::path(
    get,
    path = "/api/v1/households/{id}/tokens",
    tag = "jetons",
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, body = Vec<ApiTokenInfo>),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn list_tokens(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ApiTokenInfo>>> {
    require_owner(&state.db, id, user_id).await?;
    let rows: Vec<TokenRow> = sqlx::query_as(
        "SELECT id, name, scopes, created_at, last_used_at FROM api_tokens
         WHERE household_id = $1 ORDER BY created_at",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

/// Nouveau jeton.
#[derive(Deserialize, ToSchema)]
pub struct NewApiToken {
    pub name: String,
    pub scopes: Vec<Scope>,
}

/// Jeton créé : sa valeur n'est montrée qu'ici.
#[derive(Serialize, ToSchema)]
pub struct CreatedApiToken {
    #[serde(flatten)]
    pub info: ApiTokenInfo,
    /// À envoyer en `Authorization: Bearer …`.
    pub token: String,
}

/// Crée un jeton d'API pour le foyer (propriétaires).
#[utoipa::path(
    post,
    path = "/api/v1/households/{id}/tokens",
    tag = "jetons",
    params(("id" = Uuid, Path)),
    request_body = NewApiToken,
    responses(
        (status = 201, body = CreatedApiToken),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn create_token(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Path(id): Path<Uuid>,
    Json(body): Json<NewApiToken>,
) -> ApiResult<(StatusCode, Json<CreatedApiToken>)> {
    require_owner(&state.db, id, user_id).await?;
    let name = validate::name(&body.name)?;
    let scopes: BTreeSet<&str> = body.scopes.iter().map(|s| s.as_str()).collect();
    if scopes.is_empty() {
        return Err(ApiError::invalid("Au moins un droit."));
    }
    let token = format!("chd_{}", random_token());
    let row: TokenRow = sqlx::query_as(
        "INSERT INTO api_tokens (id, household_id, name, token_hash, scopes, created_by)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id, name, scopes, created_at, last_used_at",
    )
    .bind(Uuid::now_v7())
    .bind(id)
    .bind(name)
    .bind(sha256(token.as_bytes()))
    .bind(scopes.into_iter().collect::<Vec<_>>())
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedApiToken {
            info: row.into(),
            token,
        }),
    ))
}

/// Révoque un jeton (propriétaires).
#[utoipa::path(
    delete,
    path = "/api/v1/households/{id}/tokens/{token_id}",
    tag = "jetons",
    params(("id" = Uuid, Path), ("token_id" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn revoke_token(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Path((id, token_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    require_owner(&state.db, id, user_id).await?;
    let deleted = sqlx::query("DELETE FROM api_tokens WHERE id = $1 AND household_id = $2")
        .bind(token_id)
        .bind(id)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}
