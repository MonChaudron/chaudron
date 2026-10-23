//! Foyers, comptes du foyer (rôles) et invitations.
//!
//! Droits : tout compte du foyer voit et modifie tout ce qui concerne la cuisine ; seuls les propriétaires
//! gèrent les comptes (inviter, retirer, changer un rôle) et suppriment le foyer. Il reste toujours au
//! moins un propriétaire. Hors de son foyer, tout répond 404.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, patch, post},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection, PgPool};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::CurrentUser,
    crypto::{is_token, random_token, sha256},
    error::{ApiError, ApiResult, ErrorBody},
    types::Role,
    validate,
};

/// Durée de validité d'un lien d'invitation.
pub const INVITE_DAYS: i32 = 7;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/households", post(create))
        .route("/households/{id}", get(read).patch(update).delete(remove))
        .route(
            "/households/{id}/users/{user_id}",
            patch(change_role).delete(remove_user),
        )
        .route(
            "/households/{id}/invites",
            get(list_invites).post(create_invite),
        )
        .route(
            "/households/{id}/invites/{invite_id}",
            delete(revoke_invite),
        )
        .route("/invites/{token}", get(read_invite))
        .route("/invites/{token}/accept", post(accept))
}

// Accès

/// Rôle de l'utilisateur dans le foyer ; 404 s'il n'en fait pas partie.
pub async fn role_in(db: &PgPool, household_id: Uuid, user_id: Uuid) -> ApiResult<Role> {
    let role: Option<String> = sqlx::query_scalar(
        "SELECT role FROM household_users WHERE household_id = $1 AND user_id = $2",
    )
    .bind(household_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    role.ok_or_else(ApiError::not_found)?
        .try_into()
        .map_err(|_| ApiError::internal())
}

pub async fn require_owner(db: &PgPool, household_id: Uuid, user_id: Uuid) -> ApiResult<()> {
    match role_in(db, household_id, user_id).await? {
        Role::Owner => Ok(()),
        Role::Member => Err(ApiError::forbidden("Réservé aux propriétaires du foyer.")),
    }
}

// Foyers

/// Foyer, dans la liste des foyers d'un compte.
#[derive(Serialize, ToSchema, FromRow)]
pub struct HouseholdSummary {
    pub id: Uuid,
    pub name: String,
    /// Mon rôle dans ce foyer.
    #[sqlx(try_from = "String")]
    pub role: Role,
}

pub async fn summaries(db: &PgPool, user_id: Uuid) -> Result<Vec<HouseholdSummary>, sqlx::Error> {
    sqlx::query_as(
        "SELECT h.id, h.name, hu.role
         FROM households h JOIN household_users hu ON hu.household_id = h.id
         WHERE hu.user_id = $1 ORDER BY h.created_at",
    )
    .bind(user_id)
    .fetch_all(db)
    .await
}

/// Foyer et ses comptes.
#[derive(Serialize, ToSchema)]
pub struct Household {
    pub id: Uuid,
    pub name: String,
    pub timezone: String,
    pub locale: String,
    /// Mon rôle dans ce foyer.
    pub role: Role,
    /// Comptes qui ont accès au foyer.
    pub users: Vec<HouseholdUser>,
    /// Plafond de budget d'une semaine de menu, en centimes ; nul : aucun.
    pub weekly_budget_cents: Option<i32>,
}

#[derive(Serialize, ToSchema, FromRow)]
pub struct HouseholdUser {
    pub user_id: Uuid,
    pub display_name: String,
    pub email: Option<String>,
    #[sqlx(try_from = "String")]
    pub role: Role,
}

async fn load(db: &PgPool, id: Uuid, user_id: Uuid) -> ApiResult<Household> {
    let role = role_in(db, id, user_id).await?;
    let (name, timezone, locale, weekly_budget_cents): (String, String, String, Option<i32>) =
        sqlx::query_as(
            "SELECT name, timezone, locale, weekly_budget_cents FROM households WHERE id = $1",
        )
        .bind(id)
        .fetch_one(db)
        .await?;
    let users = sqlx::query_as(
        "SELECT u.id AS user_id, u.display_name, u.email, hu.role
         FROM household_users hu JOIN users u ON u.id = hu.user_id
         WHERE hu.household_id = $1 ORDER BY hu.created_at",
    )
    .bind(id)
    .fetch_all(db)
    .await?;
    Ok(Household {
        id,
        name,
        timezone,
        locale,
        role,
        users,
        weekly_budget_cents,
    })
}

/// Plafond de budget d'une semaine de menu du foyer.
pub async fn weekly_budget(db: &PgPool, id: Uuid) -> ApiResult<Option<i32>> {
    Ok(
        sqlx::query_scalar("SELECT weekly_budget_cents FROM households WHERE id = $1")
            .bind(id)
            .fetch_optional(db)
            .await?
            .flatten(),
    )
}

#[derive(Deserialize, ToSchema)]
pub struct NewHousehold {
    pub name: String,
    /// Fuseau IANA, `Europe/Paris` par défaut.
    pub timezone: Option<String>,
}

/// Crée un foyer ; le créateur en est propriétaire.
#[utoipa::path(
    post,
    operation_id = "create_household",
    path = "/api/v1/households",
    tag = "foyer",
    request_body = NewHousehold,
    responses(
        (status = 201, body = Household),
        (status = 422, body = ErrorBody),
    )
)]
async fn create(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Json(new): Json<NewHousehold>,
) -> ApiResult<(StatusCode, Json<Household>)> {
    let name = validate::name(&new.name)?;
    let timezone = validate::timezone(new.timezone.as_deref().unwrap_or("Europe/Paris"))?;
    let id = Uuid::now_v7();
    let mut tx = state.db.begin().await?;
    sqlx::query("INSERT INTO households (id, name, timezone) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(name)
        .bind(timezone)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO household_users (household_id, user_id, role) VALUES ($1, $2, 'owner')",
    )
    .bind(id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load(&state.db, id, user_id).await?),
    ))
}

#[utoipa::path(
    get,
    operation_id = "read_household",
    path = "/api/v1/households/{id}",
    tag = "foyer",
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, body = Household),
        (status = 404, body = ErrorBody),
    )
)]
async fn read(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Household>> {
    load(&state.db, id, user_id).await.map(Json)
}

#[derive(Deserialize, ToSchema)]
pub struct HouseholdUpdate {
    pub name: Option<String>,
    pub timezone: Option<String>,
    /// Plafond d'une semaine de menu, en centimes (de 1 à 10 000 €) ; `null` le retire, absent n'y touche pas.
    /// Propriétaires seulement.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<i32>)]
    pub weekly_budget_cents: Option<Option<i32>>,
}

/// Un champ présent (même `null`) devient `Some`, un champ absent reste `None`.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<i32>>, D::Error> {
    Option::<i32>::deserialize(d).map(Some)
}

#[utoipa::path(
    patch,
    path = "/api/v1/households/{id}",
    tag = "foyer",
    params(("id" = Uuid, Path)),
    request_body = HouseholdUpdate,
    responses(
        (status = 200, body = Household),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
async fn update(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Path(id): Path<Uuid>,
    Json(update): Json<HouseholdUpdate>,
) -> ApiResult<Json<Household>> {
    role_in(&state.db, id, user_id).await?;
    if let Some(budget) = update.weekly_budget_cents {
        require_owner(&state.db, id, user_id).await?;
        if budget.is_some_and(|b| !(100..=1_000_000).contains(&b)) {
            return Err(ApiError::invalid("Un plafond de 1 à 10 000 € par semaine."));
        }
        sqlx::query(
            "UPDATE households SET weekly_budget_cents = $2, updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .bind(budget)
        .execute(&state.db)
        .await?;
    }
    let name = update.name.as_deref().map(validate::name).transpose()?;
    let timezone = update
        .timezone
        .as_deref()
        .map(validate::timezone)
        .transpose()?;
    sqlx::query(
        "UPDATE households SET name = COALESCE($2, name), timezone = COALESCE($3, timezone),
         updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(name)
    .bind(timezone)
    .execute(&state.db)
    .await?;
    load(&state.db, id, user_id).await.map(Json)
}

/// Supprime le foyer et tout ce qu'il contient (propriétaires seulement).
#[utoipa::path(
    delete,
    operation_id = "remove_household",
    path = "/api/v1/households/{id}",
    tag = "foyer",
    params(("id" = Uuid, Path)),
    responses(
        (status = 204, description = "Supprimé"),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    )
)]
async fn remove(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    require_owner(&state.db, id, user_id).await?;
    let mut tx = state.db.begin().await?;
    // Le cahier du foyer et ses retours encore en attente partent avec lui ; ses retours publiés restent.
    sqlx::query(
        "DELETE FROM recipe_feedback WHERE household_id = $1 AND (visibility = 'household' OR status = 'pending')",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM households WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// Comptes du foyer

/// Rôles du foyer, verrouillés jusqu'à la fin de la transaction.
async fn lock_roles(conn: &mut PgConnection, household_id: Uuid) -> ApiResult<Vec<(Uuid, Role)>> {
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT user_id, role FROM household_users WHERE household_id = $1 FOR UPDATE",
    )
    .bind(household_id)
    .fetch_all(conn)
    .await?;
    rows.into_iter()
        .map(|(id, role)| Ok((id, role.try_into().map_err(|_| ApiError::internal())?)))
        .collect()
}

fn last_owner() -> ApiError {
    ApiError::conflict(
        "last_owner",
        "Le foyer doit garder au moins un propriétaire.",
    )
}

#[derive(Deserialize, ToSchema)]
pub struct RoleUpdate {
    pub role: Role,
}

/// Change le rôle d'un compte du foyer (propriétaires seulement). Il reste au moins un propriétaire.
#[utoipa::path(
    patch,
    path = "/api/v1/households/{id}/users/{user_id}",
    tag = "foyer",
    params(("id" = Uuid, Path), ("user_id" = Uuid, Path)),
    request_body = RoleUpdate,
    responses(
        (status = 200, body = Household),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, description = "Dernier propriétaire", body = ErrorBody),
    )
)]
async fn change_role(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Path((id, target)): Path<(Uuid, Uuid)>,
    Json(update): Json<RoleUpdate>,
) -> ApiResult<Json<Household>> {
    require_owner(&state.db, id, me).await?;
    let mut tx = state.db.begin().await?;
    let roles = lock_roles(&mut tx, id).await?;
    let (_, current) = roles
        .iter()
        .find(|(user, _)| *user == target)
        .ok_or_else(ApiError::not_found)?;
    let owners = roles.iter().filter(|(_, r)| *r == Role::Owner).count();
    if *current == Role::Owner && update.role == Role::Member && owners == 1 {
        return Err(last_owner());
    }
    sqlx::query(
        "UPDATE household_users SET role = $3, updated_at = now() WHERE household_id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(target)
    .bind(update.role.as_str())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    load(&state.db, id, me).await.map(Json)
}

/// Retire un compte du foyer : un propriétaire retire qui il veut, chacun peut partir.
/// Le dernier propriétaire ne peut pas partir (supprimer le foyer à la place).
#[utoipa::path(
    delete,
    path = "/api/v1/households/{id}/users/{user_id}",
    tag = "foyer",
    params(("id" = Uuid, Path), ("user_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Retiré"),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, description = "Dernier propriétaire", body = ErrorBody),
    )
)]
async fn remove_user(
    State(state): State<AppState>,
    CurrentUser(me): CurrentUser,
    Path((id, target)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    if target != me {
        require_owner(&state.db, id, me).await?;
    }
    let mut tx = state.db.begin().await?;
    let roles = lock_roles(&mut tx, id).await?;
    if !roles.iter().any(|(user, _)| *user == me) {
        return Err(ApiError::not_found());
    }
    let (_, role) = roles
        .iter()
        .find(|(user, _)| *user == target)
        .ok_or_else(ApiError::not_found)?;
    let owners = roles.iter().filter(|(_, r)| *r == Role::Owner).count();
    if *role == Role::Owner && owners == 1 {
        return Err(last_owner());
    }
    sqlx::query("DELETE FROM household_users WHERE household_id = $1 AND user_id = $2")
        .bind(id)
        .bind(target)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE members SET user_id = NULL, updated_at = now() WHERE household_id = $1 AND user_id = $2")
        .bind(id)
        .bind(target)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// Invitations

/// Lien d'invitation créé. Le lien n'est donné qu'une fois : seul son hash est conservé.
#[derive(Serialize, ToSchema)]
pub struct InviteCreated {
    pub id: Uuid,
    /// À envoyer à la personne invitée (SMS, messagerie).
    pub url: String,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
}

/// Crée un lien d'invitation à usage unique, valable 7 jours (propriétaires seulement).
#[utoipa::path(
    post,
    path = "/api/v1/households/{id}/invites",
    tag = "foyer",
    params(("id" = Uuid, Path)),
    responses(
        (status = 201, body = InviteCreated),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    )
)]
async fn create_invite(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<(StatusCode, Json<InviteCreated>)> {
    require_owner(&state.db, id, user_id).await?;
    let token = random_token();
    let invite_id = Uuid::now_v7();
    let expires_at: OffsetDateTime = sqlx::query_scalar(
        "INSERT INTO household_invites (id, household_id, token_hash, created_by, expires_at)
         VALUES ($1, $2, $3, $4, now() + make_interval(days => $5)) RETURNING expires_at",
    )
    .bind(invite_id)
    .bind(id)
    .bind(sha256(token.as_bytes()))
    .bind(user_id)
    .bind(INVITE_DAYS)
    .fetch_one(&state.db)
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(InviteCreated {
            id: invite_id,
            url: format!("{}/invitation/{token}", state.config.public_url),
            expires_at,
        }),
    ))
}

/// Invitation en attente.
#[derive(Serialize, ToSchema, FromRow)]
pub struct PendingInvite {
    pub id: Uuid,
    /// Qui l'a créée.
    pub created_by: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
}

/// Invitations ni utilisées ni expirées (propriétaires seulement).
#[utoipa::path(
    get,
    path = "/api/v1/households/{id}/invites",
    tag = "foyer",
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, body = Vec<PendingInvite>),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    )
)]
async fn list_invites(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<PendingInvite>>> {
    require_owner(&state.db, id, user_id).await?;
    let invites = sqlx::query_as(
        "SELECT i.id, u.display_name AS created_by, i.created_at, i.expires_at
         FROM household_invites i LEFT JOIN users u ON u.id = i.created_by
         WHERE i.household_id = $1 AND i.used_at IS NULL AND i.expires_at > now()
         ORDER BY i.created_at",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(invites))
}

/// Annule une invitation (propriétaires seulement).
#[utoipa::path(
    delete,
    path = "/api/v1/households/{id}/invites/{invite_id}",
    tag = "foyer",
    params(("id" = Uuid, Path), ("invite_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Annulée"),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    )
)]
async fn revoke_invite(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Path((id, invite_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    require_owner(&state.db, id, user_id).await?;
    let deleted = sqlx::query("DELETE FROM household_invites WHERE id = $1 AND household_id = $2")
        .bind(invite_id)
        .bind(id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if deleted == 0 {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

/// État d'un lien d'invitation.
#[derive(Clone, Copy, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InviteStatus {
    Pending,
    Used,
    Expired,
}

/// Ce que voit la personne qui ouvre le lien, connectée ou non.
#[derive(Serialize, ToSchema)]
pub struct InviteInfo {
    pub household_name: String,
    /// Qui a invité.
    pub invited_by: Option<String>,
    pub status: InviteStatus,
    /// Le foyer, si l'utilisateur connecté en fait déjà partie.
    pub joined_household_id: Option<Uuid>,
}

/// Lit une invitation par son jeton (page `/invitation/{token}`).
#[utoipa::path(
    get,
    path = "/api/v1/invites/{token}",
    tag = "foyer",
    security(()),
    params(("token" = String, Path)),
    responses(
        (status = 200, body = InviteInfo),
        (status = 404, body = ErrorBody),
    )
)]
async fn read_invite(
    State(state): State<AppState>,
    current: Option<CurrentUser>,
    Path(token): Path<String>,
) -> ApiResult<Json<InviteInfo>> {
    if !is_token(&token) {
        return Err(ApiError::not_found());
    }
    let row: Option<(Uuid, String, Option<String>, bool, bool)> = sqlx::query_as(
        "SELECT h.id, h.name, u.display_name, i.used_at IS NOT NULL, i.expires_at <= now()
         FROM household_invites i
         JOIN households h ON h.id = i.household_id
         LEFT JOIN users u ON u.id = i.created_by
         WHERE i.token_hash = $1",
    )
    .bind(sha256(token.as_bytes()))
    .fetch_optional(&state.db)
    .await?;
    let (household_id, household_name, invited_by, used, expired) =
        row.ok_or_else(ApiError::not_found)?;
    let joined = match current {
        Some(CurrentUser(user_id)) => role_in(&state.db, household_id, user_id).await.is_ok(),
        None => false,
    };
    let status = if used {
        InviteStatus::Used
    } else if expired {
        InviteStatus::Expired
    } else {
        InviteStatus::Pending
    };
    Ok(Json(InviteInfo {
        household_name,
        invited_by,
        status,
        joined_household_id: joined.then_some(household_id),
    }))
}

#[derive(Serialize, ToSchema)]
pub struct Joined {
    pub household_id: Uuid,
}

/// Rejoint le foyer de l'invitation, pour un utilisateur déjà connecté.
#[utoipa::path(
    post,
    path = "/api/v1/invites/{token}/accept",
    tag = "foyer",
    params(("token" = String, Path)),
    responses(
        (status = 200, body = Joined),
        (status = 410, description = "Invitation utilisée, expirée ou inconnue", body = ErrorBody),
    )
)]
async fn accept(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Path(token): Path<String>,
) -> ApiResult<Json<Joined>> {
    let gone = || {
        ApiError::new(
            StatusCode::GONE,
            "invite_invalid",
            "Ce lien d'invitation n'est plus valable.",
        )
    };
    if !is_token(&token) {
        return Err(gone());
    }
    let mut tx = state.db.begin().await?;
    let household_id = accept_invite(&mut tx, &token, user_id)
        .await?
        .ok_or_else(gone)?;
    tx.commit().await?;
    Ok(Json(Joined { household_id }))
}

/// Foyer d'une invitation encore valable.
pub async fn pending_invite(
    conn: &mut PgConnection,
    token: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT household_id FROM household_invites
         WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()",
    )
    .bind(sha256(token.as_bytes()))
    .fetch_optional(conn)
    .await
}

/// Fait entrer l'utilisateur dans le foyer de l'invitation, comme `member`. L'invitation n'est consommée
/// que si elle a servi (déjà dans le foyer : elle reste disponible pour quelqu'un d'autre).
pub async fn accept_invite(
    conn: &mut PgConnection,
    token: &str,
    user_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    let row: Option<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT id, household_id FROM household_invites
         WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now() FOR UPDATE",
    )
    .bind(sha256(token.as_bytes()))
    .fetch_optional(&mut *conn)
    .await?;
    let Some((invite_id, household_id)) = row else {
        return Ok(None);
    };
    let joined = sqlx::query(
        "INSERT INTO household_users (household_id, user_id, role) VALUES ($1, $2, 'member')
         ON CONFLICT DO NOTHING",
    )
    .bind(household_id)
    .bind(user_id)
    .execute(&mut *conn)
    .await?
    .rows_affected()
        == 1;
    if joined {
        sqlx::query(
            "UPDATE household_invites SET used_by = $2, used_at = now(), updated_at = now() WHERE id = $1",
        )
        .bind(invite_id)
        .bind(user_id)
        .execute(&mut *conn)
        .await?;
        tracing::info!(user = %user_id, household = %household_id, "invitation acceptée");
    }
    Ok(Some(household_id))
}
