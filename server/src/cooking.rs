//! Cuisson en cours du foyer, partagée entre l'Echo Show et le téléphone : une par foyer, en position absolue
//! (recette, variante, créneau, mode, phase, geste), finie au bout de 6 h sans nouvelles. La réponse porte ce qu'il
//! faut pour la lire sans autre appel : l'étape, la position, les quantités du créneau.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::{Date, OffsetDateTime};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::Caller,
    error::{ApiError, ApiResult, ErrorBody},
    menus::{self, Day, MenuSlot},
    recipes::{self, Recipe},
    types::{IngredientState, Meal, Scope, Unit, text_enum},
};

/// Au-delà, la cuisson est finie : on ne propose plus de la reprendre.
const ACTIVE_HOURS: i32 = 6;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/households/{id}/cooking",
        get(read_cooking).put(put_cooking).delete(end_cooking),
    )
}

text_enum! {
    /// `fast` : une étape par phase ; `guided` : un écran par geste.
    pub enum CookingMode {
        Fast = "fast",
        Guided = "guided",
    }
}

/// Où on en est. Les positions comptent à partir de 0 ; au-delà de la fin, la dernière.
#[derive(Deserialize, ToSchema)]
pub struct CookingInput {
    /// Slug de la recette.
    pub recipe: String,
    /// Variante ; par défaut celle du créneau, sinon la variante par défaut de la recette.
    pub variant: Option<String>,
    /// Jour du créneau du menu, avec `meal`.
    #[schema(value_type = Option<String>, format = Date)]
    pub date: Option<Day>,
    pub meal: Option<Meal>,
    pub mode: CookingMode,
    pub phase: i16,
    /// Geste de la phase, en mode pas à pas ; ignoré en mode rapide et sur une phase sans gestes.
    pub step: Option<i16>,
}

/// L'écran à lire : la phase en mode rapide, le geste en pas à pas.
#[derive(Serialize, ToSchema)]
pub struct CookingStep {
    /// Titre de la phase.
    pub title: String,
    pub text: String,
    /// Texte lu par Alexa, s'il diffère.
    pub spoken_text: Option<String>,
    pub timer_s: Option<i32>,
    pub video_url: Option<String>,
}

/// Ingrédient, en quantité pour les portions de la cuisson.
#[derive(Serialize, ToSchema)]
pub struct CookingLine {
    /// Code de l'ingrédient.
    pub ingredient: String,
    pub name: String,
    pub state: IngredientState,
    /// Nulle : « au goût ».
    pub qty: Option<f32>,
    pub unit: Unit,
    pub optional: bool,
    pub note: Option<String>,
}

/// Cuisson en cours.
#[derive(Serialize, ToSchema)]
pub struct Cooking {
    pub recipe: Recipe,
    pub variant: String,
    #[schema(value_type = Option<String>, format = Date)]
    pub date: Option<Day>,
    pub meal: Option<Meal>,
    /// Portions : celles du créneau, sinon les présents au repas (au dîner du jour sans créneau).
    pub servings: i32,
    pub mode: CookingMode,
    pub phase: i16,
    pub step: Option<i16>,
    pub phase_count: i16,
    /// Gestes de la phase en cours (0 : la phase n'en a pas).
    pub step_count: i16,
    pub current: CookingStep,
    pub ingredients: Vec<CookingLine>,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(FromRow)]
struct CookingRow {
    recipe_id: Uuid,
    variant_label: String,
    date: Option<Date>,
    meal: Option<String>,
    #[sqlx(try_from = "String")]
    mode: CookingMode,
    phase: i16,
    step: Option<i16>,
    started_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

/// Position ramenée dans les bornes de la variante : (phase, geste, nombre de phases, nombre de gestes).
fn clamp(
    recipe: &Recipe,
    variant: &str,
    mode: CookingMode,
    phase: i16,
    step: Option<i16>,
) -> (i16, Option<i16>, i16, i16) {
    let phases = recipe
        .variants
        .iter()
        .find(|v| v.label == variant)
        .map_or(&[][..], |v| &v.phases[..]);
    let phase_count = phases.len() as i16;
    let phase = phase.clamp(0, (phase_count - 1).max(0));
    let step_count = phases
        .get(phase as usize)
        .map_or(0, |p| p.steps.len() as i16);
    let step = match mode {
        CookingMode::Guided if step_count > 0 => Some(step.unwrap_or(0).clamp(0, step_count - 1)),
        _ => None,
    };
    (phase, step, phase_count, step_count)
}

/// La variante nommée si elle existe encore, sinon celle du créneau, sinon celle par défaut (la première).
fn variant_of(recipe: &Recipe, wanted: Option<&str>, slot: Option<&MenuSlot>) -> String {
    [wanted, slot.map(|s| s.variant.label.as_str())]
        .into_iter()
        .flatten()
        .find(|label| recipe.variants.iter().any(|v| v.label == *label))
        .or(recipe.variants.first().map(|v| v.label.as_str()))
        .unwrap_or_default()
        .to_owned()
}

/// Le créneau de la cuisson, s'il y en a un.
async fn slot(
    db: &PgPool,
    household_id: Uuid,
    date: Option<Day>,
    meal: Option<Meal>,
    recipe_id: Uuid,
) -> ApiResult<Option<MenuSlot>> {
    match (date, meal) {
        (Some(date), Some(meal)) => menus::slot_for(db, household_id, date, meal, recipe_id).await,
        _ => Ok(None),
    }
}

/// Le jour du foyer, dans son fuseau.
async fn today(db: &PgPool, household_id: Uuid) -> ApiResult<Day> {
    let date: Date = sqlx::query_scalar(
        "SELECT (now() AT TIME ZONE timezone)::date FROM households WHERE id = $1",
    )
    .bind(household_id)
    .fetch_one(db)
    .await?;
    Ok(Day(date))
}

async fn view(db: &PgPool, household_id: Uuid, row: CookingRow) -> ApiResult<Cooking> {
    let recipe = recipes::load_by_id(db, household_id, row.recipe_id).await?;
    let date = row.date.map(Day);
    let meal = row.meal.and_then(|m| Meal::try_from(m).ok());
    let slot = slot(db, household_id, date, meal, row.recipe_id).await?;
    let (servings, scale) = match &slot {
        Some(slot) => (slot.servings, slot.scale),
        None => {
            let day = match date {
                Some(date) => date,
                None => today(db, household_id).await?,
            };
            menus::presence_scale(db, household_id, day, meal.unwrap_or(Meal::Dinner)).await?
        }
    };
    let variant = variant_of(&recipe, Some(&row.variant_label), slot.as_ref());
    let (phase, step, phase_count, step_count) =
        clamp(&recipe, &variant, row.mode, row.phase, row.step);
    let current = recipe
        .variants
        .iter()
        .find(|v| v.label == variant)
        .and_then(|v| v.phases.get(phase as usize))
        .map(|p| match step.and_then(|s| p.steps.get(s as usize)) {
            Some(s) => CookingStep {
                title: p.title.clone(),
                text: s.text.clone(),
                spoken_text: s.spoken_text.clone(),
                timer_s: s.timer_s,
                video_url: s.video_url.clone(),
            },
            None => CookingStep {
                title: p.title.clone(),
                text: p.text.clone(),
                spoken_text: p.spoken_text.clone(),
                timer_s: p.timer_s,
                video_url: None,
            },
        })
        .ok_or_else(ApiError::not_found)?;
    let ingredients = recipe
        .ingredients
        .iter()
        .map(|l| CookingLine {
            ingredient: l.ingredient.clone(),
            name: l.name.clone(),
            state: l.state,
            qty: l.qty.map(|q| {
                if l.per_batch {
                    q
                } else {
                    q * scale.for_role(l.role)
                }
            }),
            unit: l.unit,
            optional: l.optional,
            note: l.note.clone(),
        })
        .collect();
    Ok(Cooking {
        variant,
        date,
        meal,
        servings,
        mode: row.mode,
        phase,
        step,
        phase_count,
        step_count,
        current,
        ingredients,
        started_at: row.started_at,
        updated_at: row.updated_at,
        recipe,
    })
}

/// La cuisson en cours ; 204 s'il n'y en a pas (ou plus : 6 h sans nouvelles).
#[utoipa::path(
    get,
    path = "/api/v1/households/{id}/cooking",
    tag = "cuisson",
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, body = Cooking),
        (status = 204, description = "Pas de cuisson en cours"),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn read_cooking(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    caller.require(&state.db, id, Scope::Read).await?;
    let row: Option<CookingRow> = sqlx::query_as(
        "SELECT c.recipe_id, c.variant_label, c.date, c.meal, c.mode, c.phase, c.step, c.started_at, c.updated_at
         FROM cooking_sessions c
         WHERE c.household_id = $1 AND c.updated_at > now() - make_interval(hours => $2)",
    )
    .bind(id)
    .bind(ACTIVE_HOURS)
    .fetch_optional(&state.db)
    .await?;
    Ok(match row {
        Some(row) => Json(view(&state.db, id, row).await?).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    })
}

/// Commence la cuisson ou change de place (la dernière écriture gagne). Même recette et même créneau : la même
/// cuisson continue.
#[utoipa::path(
    put,
    path = "/api/v1/households/{id}/cooking",
    tag = "cuisson",
    params(("id" = Uuid, Path)),
    request_body = CookingInput,
    responses(
        (status = 200, body = Cooking),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn put_cooking(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Json(body): Json<CookingInput>,
) -> ApiResult<Json<Cooking>> {
    caller.require(&state.db, id, Scope::CookingWrite).await?;
    if body.date.is_some() != body.meal.is_some() {
        return Err(ApiError::invalid("Le jour et le repas vont ensemble."));
    }
    if body.phase < 0 || body.step.is_some_and(|s| s < 0) {
        return Err(ApiError::invalid("Les positions commencent à 0."));
    }
    let (recipe_id, _) = recipes::find(&mut *state.db.acquire().await?, id, &body.recipe)
        .await?
        .ok_or_else(|| ApiError::invalid(format!("Recette inconnue : {}.", body.recipe)))?;
    let recipe = recipes::load_by_id(&state.db, id, recipe_id).await?;
    if let Some(label) = &body.variant
        && !recipe.variants.iter().any(|v| &v.label == label)
    {
        return Err(ApiError::invalid(format!("Variante inconnue : {label}.")));
    }
    let slot = slot(&state.db, id, body.date, body.meal, recipe_id).await?;
    let variant = variant_of(&recipe, body.variant.as_deref(), slot.as_ref());
    let (phase, step, ..) = clamp(&recipe, &variant, body.mode, body.phase, body.step);
    let row: CookingRow = sqlx::query_as(
        "WITH saved AS (
             INSERT INTO cooking_sessions (household_id, recipe_id, variant_label, date, meal, mode, phase, step)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (household_id) DO UPDATE SET
                 recipe_id = EXCLUDED.recipe_id, variant_label = EXCLUDED.variant_label, date = EXCLUDED.date,
                 meal = EXCLUDED.meal, mode = EXCLUDED.mode, phase = EXCLUDED.phase, step = EXCLUDED.step,
                 started_at = CASE
                     WHEN cooking_sessions.recipe_id = EXCLUDED.recipe_id
                      AND cooking_sessions.date IS NOT DISTINCT FROM EXCLUDED.date
                      AND cooking_sessions.meal IS NOT DISTINCT FROM EXCLUDED.meal
                      AND cooking_sessions.updated_at > now() - make_interval(hours => $9)
                     THEN cooking_sessions.started_at ELSE now() END,
                 updated_at = now()
             RETURNING *
         )
         SELECT s.recipe_id, s.variant_label, s.date, s.meal, s.mode, s.phase, s.step,
                s.started_at, s.updated_at
         FROM saved s",
    )
    .bind(id)
    .bind(recipe_id)
    .bind(&variant)
    .bind(body.date.map(|d| d.0))
    .bind(body.meal.map(Meal::as_str))
    .bind(body.mode.as_str())
    .bind(phase)
    .bind(step)
    .bind(ACTIVE_HOURS)
    .fetch_one(&state.db)
    .await?;
    let cooking = view(&state.db, id, row).await?;
    state.notify(id, "cooking", None, None);
    Ok(Json(cooking))
}

/// Termine la cuisson en cours (rien à faire s'il n'y en a pas).
#[utoipa::path(
    delete,
    path = "/api/v1/households/{id}/cooking",
    tag = "cuisson",
    params(("id" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn end_cooking(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    caller.require(&state.db, id, Scope::CookingWrite).await?;
    let deleted = sqlx::query("DELETE FROM cooking_sessions WHERE household_id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() > 0 {
        state.notify(id, "cooking", None, None);
    }
    Ok(StatusCode::NO_CONTENT)
}
