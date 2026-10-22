//! Notifications dans l'appli : l'auteur d'une proposition ou d'un retour apprend ce qu'il est devenu (« “Hachis
//! parmentier” a été amélioré avec ton retour »). Une liste et un badge ; pas d'e-mail.

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::CurrentUser,
    error::{ApiResult, ErrorBody},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/notifications", get(list_notifications))
        .route("/me/notifications/read", post(mark_read))
}

/// Une notification.
#[derive(Serialize, ToSchema, FromRow)]
pub struct Notification {
    pub id: Uuid,
    /// `submission_accepted`, `submission_refused`, `feedback_applied`, `feedback_published`, `feedback_closed`,
    /// `feedback_refused`.
    pub kind: String,
    pub text: String,
    /// La recette concernée, si elle existe encore.
    pub recipe_slug: Option<String>,
    pub read: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// Prévient un compte (rien sans compte : un jeton sans auteur, un compte supprimé).
pub(crate) async fn notify(
    conn: &mut PgConnection,
    user_id: Option<Uuid>,
    kind: &str,
    recipe_id: Option<Uuid>,
    text: &str,
) -> ApiResult<()> {
    let Some(user_id) = user_id else {
        return Ok(());
    };
    sqlx::query(
        "INSERT INTO notifications (id, user_id, kind, recipe_id, text) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(kind)
    .bind(recipe_id)
    .bind(text)
    .execute(conn)
    .await?;
    Ok(())
}

/// Notifications pas encore lues (le badge).
pub(crate) async fn unread(conn: &mut PgConnection, user_id: Uuid) -> ApiResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM notifications WHERE user_id = $1 AND read_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(conn)
    .await?)
}

/// Les 50 dernières notifications du compte.
#[utoipa::path(
    get,
    path = "/api/v1/me/notifications",
    tag = "compte",
    responses(
        (status = 200, body = Vec<Notification>),
        (status = 401, body = ErrorBody),
    )
)]
pub async fn list_notifications(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
) -> ApiResult<Json<Vec<Notification>>> {
    Ok(Json(
        sqlx::query_as(
            "SELECT n.id, n.kind, n.text, r.slug AS recipe_slug, n.read_at IS NOT NULL AS read, n.created_at
             FROM notifications n LEFT JOIN recipes r ON r.id = n.recipe_id
             WHERE n.user_id = $1 ORDER BY n.created_at DESC LIMIT 50",
        )
        .bind(user_id)
        .fetch_all(&state.db)
        .await?,
    ))
}

/// Lues : celles-ci, ou toutes sans liste.
#[derive(Deserialize, ToSchema)]
pub struct MarkRead {
    #[serde(default)]
    pub ids: Vec<Uuid>,
}

#[utoipa::path(
    post,
    path = "/api/v1/me/notifications/read",
    tag = "compte",
    request_body = MarkRead,
    responses(
        (status = 204),
        (status = 401, body = ErrorBody),
    )
)]
pub async fn mark_read(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Json(body): Json<MarkRead>,
) -> ApiResult<StatusCode> {
    sqlx::query(
        "UPDATE notifications SET read_at = now()
         WHERE user_id = $1 AND read_at IS NULL AND (cardinality($2::uuid[]) = 0 OR id = ANY($2))",
    )
    .bind(user_id)
    .bind(&body.ids)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
