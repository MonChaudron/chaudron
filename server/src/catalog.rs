//! Catalogue des équipements et équipements du foyer.

use std::collections::BTreeSet;

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::CurrentUser,
    error::{ApiError, ApiResult, ErrorBody},
    households::role_in,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/equipment", get(equipment)).route(
        "/households/{id}/equipment",
        get(household_equipment).put(set_household_equipment),
    )
}

/// Appareil du catalogue.
#[derive(Serialize, ToSchema, FromRow)]
pub struct Equipment {
    /// Ex. `cookeo_6l`.
    pub code: String,
    /// Clé de traduction du nom.
    pub name_key: String,
    /// Contraintes de l'appareil, ex. `{"max_fill_l": 4, "min_liquid_ml": 250}`.
    #[schema(value_type = Object)]
    pub rules: Value,
}

/// Catalogue des équipements.
#[utoipa::path(
    get,
    path = "/api/v1/equipment",
    tag = "référentiels",
    responses((status = 200, body = Vec<Equipment>))
)]
async fn equipment(
    State(state): State<AppState>,
    _: CurrentUser,
) -> ApiResult<Json<Vec<Equipment>>> {
    let list = sqlx::query_as("SELECT code, name_key, rules FROM equipment ORDER BY code")
        .fetch_all(&state.db)
        .await?;
    Ok(Json(list))
}

/// Équipements que possède le foyer.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct HouseholdEquipment {
    /// Codes du catalogue.
    pub codes: Vec<String>,
}

async fn load_codes(db: &PgPool, household_id: Uuid) -> ApiResult<HouseholdEquipment> {
    let codes = sqlx::query_scalar(
        "SELECT equipment_code FROM household_equipment WHERE household_id = $1 ORDER BY equipment_code",
    )
    .bind(household_id)
    .fetch_all(db)
    .await?;
    Ok(HouseholdEquipment { codes })
}

#[utoipa::path(
    get,
    path = "/api/v1/households/{id}/equipment",
    tag = "foyer",
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, body = HouseholdEquipment),
        (status = 404, body = ErrorBody),
    )
)]
async fn household_equipment(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<HouseholdEquipment>> {
    role_in(&state.db, id, user_id).await?;
    load_codes(&state.db, id).await.map(Json)
}

/// Remplace la liste des équipements du foyer.
#[utoipa::path(
    put,
    path = "/api/v1/households/{id}/equipment",
    tag = "foyer",
    params(("id" = Uuid, Path)),
    request_body = HouseholdEquipment,
    responses(
        (status = 200, body = HouseholdEquipment),
        (status = 404, body = ErrorBody),
        (status = 422, description = "Équipement inconnu", body = ErrorBody),
    )
)]
async fn set_household_equipment(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Path(id): Path<Uuid>,
    Json(body): Json<HouseholdEquipment>,
) -> ApiResult<Json<HouseholdEquipment>> {
    role_in(&state.db, id, user_id).await?;
    let codes: Vec<String> = body
        .codes
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let known: i64 = sqlx::query_scalar("SELECT count(*) FROM equipment WHERE code = ANY($1)")
        .bind(&codes)
        .fetch_one(&state.db)
        .await?;
    if known != codes.len() as i64 {
        return Err(ApiError::invalid("Équipement inconnu."));
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM household_equipment WHERE household_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO household_equipment (household_id, equipment_code) SELECT $1, * FROM UNNEST($2::text[])",
    )
    .bind(id)
    .bind(&codes)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    load_codes(&state.db, id).await.map(Json)
}
