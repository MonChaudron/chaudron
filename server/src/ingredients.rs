//! Référentiel d'ingrédients et table Ciqual : ce que manipulent les recettes (« Carotte »), relié par forme
//! d'achat aux lignes Ciqual (« Carotte, crue », « Carotte, surgelée, crue »).

use std::collections::HashSet;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection, PgExecutor, PgPool};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{Caller, CurrentUser},
    error::{ApiError, ApiResult, ErrorBody},
    text::{code, like_patterns, search_key},
    types::{Aisle, IngredientState, Scope, Unit},
    validate,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/ingredients",
            get(list_ingredients).post(create_ingredient),
        )
        .route("/ingredients/{id}/foods/{state}", put(link_food))
        .route("/foods", get(search_foods))
}

/// Ligne Ciqual d'un ingrédient pour une forme d'achat.
#[derive(Clone, Serialize, ToSchema)]
pub struct IngredientFood {
    pub state: IngredientState,
    /// Code Ciqual.
    pub food_code: i32,
    /// Nom Ciqual (absent si la ligne n'est pas chargée).
    pub food_name: Option<String>,
}

/// Ingrédient du référentiel.
#[derive(Serialize, ToSchema)]
pub struct Ingredient {
    pub id: Uuid,
    /// Tiré du nom à la création, stable ensuite.
    pub code: String,
    pub name: String,
    pub aisle: Aisle,
    pub default_unit: Unit,
    /// Forme d'achat proposée par défaut dans une recette.
    pub default_state: IngredientState,
    /// Jours au réfrigérateur, frais, quand ils diffèrent de ceux du rayon (le haché : 1).
    pub keeps_days: Option<i16>,
    /// Lignes Ciqual par forme d'achat (frais, surgelé, conserve, sec, cuit), dans cet ordre.
    pub foods: Vec<IngredientFood>,
}

#[derive(FromRow)]
struct IngredientRow {
    id: Uuid,
    code: String,
    name: String,
    #[sqlx(try_from = "String")]
    aisle: Aisle,
    #[sqlx(try_from = "String")]
    default_unit: Unit,
    #[sqlx(try_from = "String")]
    default_state: IngredientState,
    keeps_days: Option<i16>,
}

#[derive(FromRow)]
struct FoodLinkRow {
    ingredient_id: Uuid,
    #[sqlx(try_from = "String")]
    state: IngredientState,
    food_code: i32,
    food_name: Option<String>,
}

/// Ingrédients (tous, ou un seul), avec leurs lignes Ciqual.
async fn load(db: &PgPool, only: Option<Uuid>) -> ApiResult<Vec<Ingredient>> {
    let rows: Vec<IngredientRow> = sqlx::query_as(
        "SELECT id, code, name, aisle, default_unit, default_state, keeps_days FROM ingredients
         WHERE $1::uuid IS NULL OR id = $1 ORDER BY search_key",
    )
    .bind(only)
    .fetch_all(db)
    .await?;
    let links: Vec<FoodLinkRow> = sqlx::query_as(
        "SELECT l.ingredient_id, l.state, l.food_code, f.name AS food_name
         FROM ingredient_foods l LEFT JOIN foods f ON f.code = l.food_code
         WHERE $1::uuid IS NULL OR l.ingredient_id = $1
         ORDER BY array_position(ARRAY['fresh', 'frozen', 'canned', 'dried', 'cooked'], l.state)",
    )
    .bind(only)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Ingredient {
            foods: links
                .iter()
                .filter(|l| l.ingredient_id == r.id)
                .map(|l| IngredientFood {
                    state: l.state,
                    food_code: l.food_code,
                    food_name: l.food_name.clone(),
                })
                .collect(),
            id: r.id,
            code: r.code,
            name: r.name,
            aisle: r.aisle,
            default_unit: r.default_unit,
            default_state: r.default_state,
            keeps_days: r.keeps_days,
        })
        .collect())
}

pub async fn load_one(db: &PgPool, id: Uuid) -> ApiResult<Ingredient> {
    load(db, Some(id))
        .await?
        .pop()
        .ok_or_else(ApiError::not_found)
}

/// Référentiel complet des ingrédients, trié par nom (la recherche se fait côté client).
#[utoipa::path(
    get,
    path = "/api/v1/ingredients",
    tag = "référentiels",
    responses((status = 200, body = Vec<Ingredient>))
)]
pub async fn list_ingredients(
    State(state): State<AppState>,
    caller: Caller,
) -> ApiResult<Json<Vec<Ingredient>>> {
    caller.require_scope(Scope::Read)?;
    load(&state.db, None).await.map(Json)
}

/// Ligne Ciqual à relier à une forme d'achat.
#[derive(Deserialize, ToSchema)]
pub struct FoodLink {
    pub state: IngredientState,
    pub food_code: i32,
}

/// Nouvel ingrédient.
#[derive(Deserialize, ToSchema)]
pub struct NewIngredient {
    pub name: String,
    pub aisle: Aisle,
    pub default_unit: Unit,
    pub default_state: IngredientState,
    /// Jours au réfrigérateur, frais, quand ils diffèrent de ceux du rayon (poisson 2, boucherie et boulangerie 3,
    /// fruits et légumes, crèmerie et frais 7). Dans un lot, complète aussi un ingrédient qui existe déjà et n'en a
    /// pas (le référentiel est commun à l'instance : rien n'est remplacé).
    pub keeps_days: Option<i16>,
    /// Lignes Ciqual, au plus une par forme d'achat.
    #[serde(default)]
    pub foods: Vec<FoodLink>,
}

async fn check_food(db: impl PgExecutor<'_>, food_code: i32) -> ApiResult<()> {
    let known: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM foods WHERE code = $1)")
        .bind(food_code)
        .fetch_one(db)
        .await?;
    if !known {
        return Err(ApiError::invalid("Aliment Ciqual inconnu."));
    }
    Ok(())
}

/// Crée un ingrédient (à partir d'une ligne Ciqual, ou sans). Son code est tiré du nom.
#[utoipa::path(
    post,
    path = "/api/v1/ingredients",
    tag = "référentiels",
    request_body = NewIngredient,
    responses(
        (status = 201, body = Ingredient),
        (status = 409, description = "Un ingrédient porte déjà ce nom", body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn create_ingredient(
    State(state): State<AppState>,
    _: CurrentUser,
    Json(body): Json<NewIngredient>,
) -> ApiResult<(StatusCode, Json<Ingredient>)> {
    let mut tx = state.db.begin().await?;
    let id = insert(&mut tx, &body).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(load_one(&state.db, id).await?)))
}

/// Crée un ingrédient et ses liens Ciqual ; utilisé aussi par l'import en lot.
pub async fn insert(conn: &mut PgConnection, body: &NewIngredient) -> ApiResult<Uuid> {
    let name = validate::name(&body.name)?;
    let code = code(name);
    if code.is_empty() {
        return Err(ApiError::invalid("Le nom doit contenir des lettres."));
    }
    if body.keeps_days.is_some_and(|d| !(1..=60).contains(&d)) {
        return Err(ApiError::invalid(
            "Un ingrédient frais tient de 1 à 60 jours.",
        ));
    }
    let mut states = HashSet::new();
    if !body.foods.iter().all(|f| states.insert(f.state)) {
        return Err(ApiError::invalid("Une seule ligne Ciqual par état."));
    }
    for food in &body.foods {
        check_food(&mut *conn, food.food_code).await?;
    }
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO ingredients (id, code, name, search_key, aisle, default_unit, default_state, keeps_days)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(&code)
    .bind(name)
    .bind(search_key(name))
    .bind(body.aisle.as_str())
    .bind(body.default_unit.as_str())
    .bind(body.default_state.as_str())
    .bind(body.keeps_days)
    .execute(&mut *conn)
    .await
    .map_err(|err| match ApiError::from(err) {
        e if e.is_conflict() => ApiError::conflict("conflict", "Cet ingrédient existe déjà."),
        e => e,
    })?;
    for food in &body.foods {
        sqlx::query(
            "INSERT INTO ingredient_foods (ingredient_id, state, food_code) VALUES ($1, $2, $3)",
        )
        .bind(id)
        .bind(food.state.as_str())
        .bind(food.food_code)
        .execute(&mut *conn)
        .await?;
    }
    Ok(id)
}

/// Ligne Ciqual d'une forme d'achat.
#[derive(Deserialize, ToSchema)]
pub struct FoodCode {
    pub food_code: i32,
}

/// Relie une forme d'achat de l'ingrédient à une ligne Ciqual (remplace la précédente).
#[utoipa::path(
    put,
    path = "/api/v1/ingredients/{id}/foods/{state}",
    tag = "référentiels",
    params(("id" = Uuid, Path), ("state" = IngredientState, Path)),
    request_body = FoodCode,
    responses(
        (status = 200, body = Ingredient),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn link_food(
    State(state): State<AppState>,
    _: CurrentUser,
    Path((id, ingredient_state)): Path<(Uuid, IngredientState)>,
    Json(body): Json<FoodCode>,
) -> ApiResult<Json<Ingredient>> {
    load_one(&state.db, id).await?;
    check_food(&state.db, body.food_code).await?;
    sqlx::query(
        "INSERT INTO ingredient_foods (ingredient_id, state, food_code) VALUES ($1, $2, $3)
         ON CONFLICT (ingredient_id, state) DO UPDATE SET food_code = excluded.food_code, updated_at = now()",
    )
    .bind(id)
    .bind(ingredient_state.as_str())
    .bind(body.food_code)
    .execute(&state.db)
    .await?;
    load_one(&state.db, id).await.map(Json)
}

/// Aliment de la table Ciqual, apports pour 100 g (nuls si non renseignés).
#[derive(Serialize, ToSchema, FromRow)]
pub struct Food {
    pub code: i32,
    pub name: String,
    pub group_code: String,
    pub subgroup_code: String,
    pub energy_kcal: Option<f32>,
    pub protein_g: Option<f32>,
    pub carbs_g: Option<f32>,
    pub sugars_g: Option<f32>,
    pub fat_g: Option<f32>,
    pub sat_fat_g: Option<f32>,
    pub fiber_g: Option<f32>,
    pub salt_g: Option<f32>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct FoodQuery {
    /// Mots cherchés, tous présents, sans tenir compte des accents ni de la casse.
    pub q: String,
    /// 20 par défaut, 50 au plus.
    pub limit: Option<i64>,
}

/// Recherche dans la table Ciqual : les noms qui commencent par le premier mot d'abord, puis les plus courts.
#[utoipa::path(
    get,
    path = "/api/v1/foods",
    tag = "référentiels",
    params(FoodQuery),
    responses((status = 200, body = Vec<Food>))
)]
pub async fn search_foods(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<FoodQuery>,
) -> ApiResult<Json<Vec<Food>>> {
    caller.require_scope(Scope::Read)?;
    let patterns = like_patterns(&query.q);
    // Le premier mot en début de nom : « %carot% » → « carot% ».
    let Some(first) = patterns.first().map(|p| p[1..].to_owned()) else {
        return Ok(Json(Vec::new()));
    };
    let foods = sqlx::query_as(
        "SELECT code, name, group_code, subgroup_code, energy_kcal, protein_g, carbs_g, sugars_g, fat_g,
                sat_fat_g, fiber_g, salt_g
         FROM foods WHERE search_key LIKE ALL($1)
         ORDER BY search_key LIKE $2 DESC, length(name), name LIMIT $3",
    )
    .bind(&patterns)
    .bind(first)
    .bind(query.limit.unwrap_or(20).clamp(1, 50))
    .fetch_all(&state.db)
    .await?;
    Ok(Json(foods))
}
