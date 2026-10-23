//! À revoir › Prix : ce que l'admin d'instance corrige. Pas de prix saisi à la main : on corrige le rattachement
//! d'un ingrédient (catégorie, série, poids d'une pièce), puis il est rechiffré.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::Caller,
    error::{ApiError, ApiResult, ErrorBody},
    ingredients::{self, Ingredient},
    types::{IngredientState, text_enum},
};

use super::{PriceLink, PriceUnit, run};

text_enum! {
    /// Ce qui ne va pas avec le prix d'un ingrédient.
    pub enum IssueKind {
        /// Open Prices et l'INSEE s'écartent de plus de moitié : le rattachement est sans doute faux.
        Divergent = "divergent",
        /// Rattaché, mais aucun prix trouvé.
        Missing = "missing",
        /// Utilisé par une recette, sans rattachement pour cette forme d'achat.
        Unlinked = "unlinked",
    }
}

/// Un ingrédient à revoir, dans une forme d'achat.
#[derive(Serialize, ToSchema, FromRow)]
pub struct PriceIssue {
    pub ingredient_id: Uuid,
    /// Code de l'ingrédient.
    pub ingredient: String,
    pub name: String,
    #[sqlx(try_from = "String")]
    pub state: IngredientState,
    #[sqlx(try_from = "String")]
    pub kind: IssueKind,
    pub cents: Option<i32>,
    pub insee_cents: Option<i32>,
    #[sqlx(skip)]
    pub link: Option<PriceLink>,
}

/// Le dernier passage de la mise à jour.
#[derive(Serialize, ToSchema, FromRow)]
pub struct PriceRun {
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub finished_at: Option<OffsetDateTime>,
    pub ok: bool,
    pub priced: i32,
    pub message: String,
}

#[derive(Serialize, ToSchema)]
pub struct PriceReview {
    pub issues: Vec<PriceIssue>,
    pub last_run: Option<PriceRun>,
}

/// Les ingrédients dont le prix est à revoir, et le dernier passage de la mise à jour. Admin de l'instance.
#[utoipa::path(
    get,
    path = "/api/v1/community/prices",
    tag = "communauté",
    responses((status = 200, body = PriceReview), (status = 403, body = ErrorBody))
)]
pub async fn review(State(state): State<AppState>, caller: Caller) -> ApiResult<Json<PriceReview>> {
    caller.require_admin(&state).await?;
    let mut issues: Vec<PriceIssue> = sqlx::query_as(
        "SELECT i.id AS ingredient_id, i.code AS ingredient, i.name, p.state, 'divergent' AS kind, p.cents, p.insee_cents
         FROM ingredient_prices p JOIN ingredients i ON i.id = p.ingredient_id WHERE p.flag = 'divergent'
         UNION ALL
         SELECT i.id, i.code, i.name, l.state, 'missing', NULL, NULL
         FROM ingredient_price_links l JOIN ingredients i ON i.id = l.ingredient_id
         WHERE NOT EXISTS (SELECT 1 FROM ingredient_prices p WHERE p.ingredient_id = l.ingredient_id AND p.state = l.state)
         UNION ALL
         SELECT DISTINCT i.id, i.code, i.name, r.state, 'unlinked', NULL::int, NULL::int
         FROM recipe_ingredients r JOIN ingredients i ON i.id = r.ingredient_id
         WHERE NOT EXISTS (SELECT 1 FROM ingredient_price_links l WHERE l.ingredient_id = r.ingredient_id AND l.state = r.state)
         ORDER BY kind, ingredient",
    )
    .fetch_all(&state.db)
    .await?;
    let links = super::links(&state.db, None).await?;
    for issue in &mut issues {
        issue.link = links
            .iter()
            .find(|(id, l)| *id == issue.ingredient_id && l.state == issue.state)
            .map(|(_, l)| l.clone());
    }
    let last_run: Option<PriceRun> = sqlx::query_as(
        "SELECT started_at, finished_at, ok, priced, message FROM price_runs ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await?;
    Ok(Json(PriceReview { issues, last_run }))
}

/// Correction d'un rattachement par l'admin : remplace celui de cette forme d'achat.
#[derive(Deserialize, ToSchema)]
pub struct LinkUpdate {
    pub off_category: Option<String>,
    pub off_product_category: Option<String>,
    pub insee_series: Option<String>,
    pub insee_per: Option<PriceUnit>,
    pub insee_index: Option<String>,
    /// Poids d'une pièce, en grammes ; remplace l'ancien s'il est donné.
    pub piece_g: Option<f32>,
    /// Densité (g/ml) ; remplace l'ancienne si elle est donnée.
    pub density: Option<f32>,
}

fn clean(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

/// Remplace le rattachement d'un ingrédient pour une forme d'achat (le seul chemin qui le remplace), puis le rechiffre
/// en tâche détachée. Admin de l'instance.
#[utoipa::path(
    put,
    path = "/api/v1/community/ingredients/{id}/price-links/{state}",
    tag = "communauté",
    params(("id" = Uuid, Path), ("state" = String, Path)),
    request_body = LinkUpdate,
    responses(
        (status = 200, body = Ingredient),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn put_link(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, ingredient_state)): Path<(Uuid, String)>,
    Json(update): Json<LinkUpdate>,
) -> ApiResult<Json<Ingredient>> {
    caller.require_admin(&state).await?;
    let ingredient_state = IngredientState::try_from(ingredient_state)
        .map_err(|_| ApiError::invalid("Forme d'achat inconnue."))?;
    let invalid = |v: f32| v.is_nan() || v <= 0.0;
    if update.piece_g.is_some_and(invalid) || update.density.is_some_and(invalid) {
        return Err(ApiError::invalid(
            "Poids d'une pièce et densité sont positifs.",
        ));
    }
    ingredients::load_one(&state.db, id).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "INSERT INTO ingredient_price_links
             (ingredient_id, state, off_category, off_product_category, insee_series, insee_per, insee_index)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (ingredient_id, state) DO UPDATE SET
             off_category = excluded.off_category, off_product_category = excluded.off_product_category,
             insee_series = excluded.insee_series, insee_per = excluded.insee_per,
             insee_index = excluded.insee_index, updated_at = now()",
    )
    .bind(id)
    .bind(ingredient_state.as_str())
    .bind(clean(&update.off_category))
    .bind(clean(&update.off_product_category))
    .bind(clean(&update.insee_series))
    .bind(update.insee_per.map(PriceUnit::as_str))
    .bind(clean(&update.insee_index))
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE ingredients SET piece_g = coalesce($2, piece_g), density = coalesce($3, density) WHERE id = $1",
    )
    .bind(id)
    .bind(update.piece_g)
    .bind(update.density)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    let detached = state.clone();
    tokio::spawn(async move {
        match run::price_ingredients(&detached, Some(&[id])).await {
            Ok(outcome) if outcome.errors.is_empty() => {}
            Ok(outcome) => {
                tracing::warn!(message = %outcome.message(), "chiffrage d'un rattachement corrigé")
            }
            Err(err) => tracing::warn!(%err, "chiffrage d'un rattachement corrigé"),
        }
    });
    ingredients::load_one(&state.db, id).await.map(Json)
}

/// Relance un passage complet de la mise à jour, en tâche détachée. Admin de l'instance.
#[utoipa::path(
    post,
    path = "/api/v1/community/prices/refresh",
    tag = "communauté",
    responses((status = 202, description = "Lancée"), (status = 403, body = ErrorBody))
)]
pub async fn refresh(State(state): State<AppState>, caller: Caller) -> ApiResult<StatusCode> {
    caller.require_admin(&state).await?;
    tokio::spawn(async move {
        if let Err(err) = run::run(&state).await {
            tracing::warn!(%err, "mise à jour des prix relancée");
        }
    });
    Ok(StatusCode::ACCEPTED)
}
