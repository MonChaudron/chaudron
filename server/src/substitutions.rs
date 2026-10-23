//! Remplacements d'ingrédients : « lieu noir pour cabillaud, × 1,1, aussi bon ». Saisis par l'admin de l'instance
//! (ou issus d'une variante tentée validée), proposés sur la liste de courses dans toutes les recettes qui ont
//! l'ingrédient ; une recette les précise ou les refuse.

use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::Caller,
    error::{ApiError, ApiResult, ErrorBody},
    notifications,
    recipes::{self, Recipe},
    types::{IngredientState, Scope},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/substitutions", get(list_substitutions))
        .route("/community/substitutions", post(create_substitution))
        .route(
            "/community/substitutions/{substitution_id}",
            put(update_substitution).delete(delete_substitution),
        )
        .route(
            "/households/{id}/recipes/{slug}/substitutions/{substitution_id}",
            put(set_rule).delete(delete_rule),
        )
}

/// Un remplacement.
#[derive(Serialize, ToSchema, FromRow)]
pub struct Substitution {
    pub id: Uuid,
    /// Code de l'ingrédient remplacé.
    pub ingredient: String,
    pub ingredient_name: String,
    /// Code du remplaçant.
    pub substitute: String,
    pub substitute_name: String,
    /// Forme d'achat du remplaçant (`fresh`, `frozen`…) ; nulle : celle de la ligne remplacée.
    pub substitute_state: Option<String>,
    /// Quantité du remplaçant pour une quantité de l'ingrédient.
    pub factor: f32,
    pub note: String,
    /// `admin` ou `feedback` (une variante tentée validée).
    pub source: String,
}

const SELECT: &str = "SELECT s.id, i.code AS ingredient, i.name AS ingredient_name, r.code AS substitute,
        r.name AS substitute_name, s.substitute_state, s.factor, s.note, s.source
    FROM substitutions s JOIN ingredients i ON i.id = s.ingredient_id JOIN ingredients r ON r.id = s.substitute_id";

/// Ce qu'une recette dit d'un remplacement.
#[derive(Serialize, ToSchema, FromRow, Clone)]
pub struct RecipeSubstitution {
    pub substitution_id: Uuid,
    /// Refusé dans cette recette (« pas dans le tartare »).
    pub allowed: bool,
    /// Une précision (« +2 min de vapeur »).
    pub note: String,
}

pub(crate) async fn rules(
    conn: &mut PgConnection,
    recipe_id: Uuid,
) -> ApiResult<Vec<RecipeSubstitution>> {
    Ok(sqlx::query_as(
        "SELECT substitution_id, allowed, note FROM recipe_substitutions WHERE recipe_id = $1",
    )
    .bind(recipe_id)
    .fetch_all(conn)
    .await?)
}

/// Un remplacement proposé sur une ligne de courses.
#[derive(Serialize, ToSchema, Clone, Debug)]
pub struct Offer {
    /// Le remplacement.
    pub id: Uuid,
    /// Code du remplaçant.
    pub ingredient: String,
    pub label: String,
    /// Quantité à prendre, pour toute la ligne.
    pub qty: Option<f32>,
    /// La précision de la recette, sinon celle du remplacement.
    pub note: String,
    /// Rayon du remplaçant.
    #[serde(skip)]
    pub(crate) aisle: String,
    /// Le remplaçant, et sa forme d'achat quand elle diffère de celle de la ligne remplacée.
    #[serde(skip)]
    pub(crate) substitute_id: Uuid,
    #[serde(skip)]
    pub(crate) substitute_state: Option<IngredientState>,
    /// Prix estimé de la quantité à prendre.
    pub price_cents: Option<i32>,
}

/// Un remplacement d'un ingrédient.
struct Entry {
    id: Uuid,
    substitute_id: Uuid,
    substitute_state: Option<IngredientState>,
    code: String,
    name: String,
    aisle: String,
    factor: f32,
    note: String,
}

/// Les remplacements de ces ingrédients, par ingrédient remplacé ; et ce qu'en disent ces recettes, par (recette,
/// remplacement).
pub(crate) struct Catalog {
    by_ingredient: HashMap<Uuid, Vec<Entry>>,
    rules: HashMap<(Uuid, Uuid), RecipeSubstitution>,
}

impl Catalog {
    pub(crate) async fn load(
        conn: &mut PgConnection,
        ingredients: &[Uuid],
        recipes: &[Uuid],
    ) -> ApiResult<Self> {
        type Row = (
            Uuid,
            Uuid,
            Uuid,
            Option<String>,
            String,
            String,
            String,
            f32,
            String,
        );
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT s.id, s.ingredient_id, s.substitute_id, s.substitute_state, r.code, r.name, r.aisle, s.factor, s.note
             FROM substitutions s JOIN ingredients r ON r.id = s.substitute_id
             WHERE s.ingredient_id = ANY($1) ORDER BY r.search_key",
        )
        .bind(ingredients)
        .fetch_all(&mut *conn)
        .await?;
        let mut by_ingredient: HashMap<Uuid, Vec<_>> = HashMap::new();
        for (id, ingredient, substitute_id, state, code, name, aisle, factor, note) in rows {
            by_ingredient.entry(ingredient).or_default().push(Entry {
                id,
                substitute_id,
                substitute_state: state.and_then(|s| IngredientState::try_from(s).ok()),
                code,
                name,
                aisle,
                factor,
                note,
            });
        }
        let rules: Vec<(Uuid, Uuid, bool, String)> = sqlx::query_as(
            "SELECT recipe_id, substitution_id, allowed, note FROM recipe_substitutions WHERE recipe_id = ANY($1)",
        )
        .bind(recipes)
        .fetch_all(&mut *conn)
        .await?;
        Ok(Self {
            by_ingredient,
            rules: rules
                .into_iter()
                .map(|(recipe, substitution_id, allowed, note)| {
                    (
                        (recipe, substitution_id),
                        RecipeSubstitution {
                            substitution_id,
                            allowed,
                            note,
                        },
                    )
                })
                .collect(),
        })
    }

    /// Les remplaçants de tous les ingrédients du catalogue.
    pub(crate) fn substitute_ids(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.by_ingredient
            .values()
            .flatten()
            .map(|e| e.substitute_id)
    }

    /// Les remplacements d'un ingrédient dans ces recettes : aucun qu'une des recettes refuse. `qty` : la quantité
    /// de l'ingrédient, multipliée par le coefficient.
    pub(crate) fn offers(
        &self,
        ingredient: Uuid,
        recipes: &[Uuid],
        qty: Option<f32>,
    ) -> Vec<Offer> {
        self.by_ingredient
            .get(&ingredient)
            .into_iter()
            .flatten()
            .filter(|e| {
                !recipes.iter().any(|r| {
                    self.rules
                        .get(&(*r, e.id))
                        .is_some_and(|rule| !rule.allowed)
                })
            })
            .map(|e| Offer {
                id: e.id,
                ingredient: e.code.clone(),
                label: e.name.clone(),
                qty: qty.map(|q| (q * e.factor * 10.0).round() / 10.0),
                note: recipes
                    .iter()
                    .filter_map(|r| self.rules.get(&(*r, e.id)))
                    .map(|rule| rule.note.clone())
                    .find(|n| !n.is_empty())
                    .unwrap_or_else(|| e.note.clone()),
                aisle: e.aisle.clone(),
                substitute_id: e.substitute_id,
                substitute_state: e.substitute_state,
                price_cents: None,
            })
            .collect()
    }
}

// Routes

/// Tous les remplacements de l'instance (la copie de travail du téléphone : chercher une promo hors ligne).
#[utoipa::path(
    get,
    path = "/api/v1/substitutions",
    tag = "référentiels",
    responses((status = 200, body = Vec<Substitution>))
)]
pub async fn list_substitutions(
    State(state): State<AppState>,
    caller: Caller,
) -> ApiResult<Json<Vec<Substitution>>> {
    caller.require_scope(Scope::Read)?;
    Ok(Json(
        sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "{SELECT} ORDER BY i.search_key, r.search_key"
        )))
        .fetch_all(&state.db)
        .await?,
    ))
}

/// Un remplacement à saisir.
#[derive(Deserialize, ToSchema)]
pub struct SubstitutionInput {
    /// Code de l'ingrédient remplacé.
    pub ingredient: String,
    /// Code du remplaçant.
    pub substitute: String,
    pub substitute_state: Option<IngredientState>,
    pub factor: f32,
    pub note: Option<String>,
    /// La variante tentée qu'il valide : elle passe en « appliquée », son auteur est prévenu.
    pub feedback_id: Option<Uuid>,
}

async fn fetch(conn: &mut PgConnection, id: Uuid) -> ApiResult<Substitution> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("{SELECT} WHERE s.id = $1")))
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(ApiError::not_found)
}

fn check(factor: f32, note: Option<&str>) -> ApiResult<String> {
    if !(factor > 0.0 && factor <= 10.0) {
        return Err(ApiError::invalid("Coefficient entre 0 et 10."));
    }
    let note = note.unwrap_or_default().trim();
    if note.chars().count() > 300 {
        return Err(ApiError::invalid("La note fait 300 caractères au plus."));
    }
    Ok(note.to_owned())
}

/// Saisit un remplacement (admin), éventuellement depuis une variante tentée.
#[utoipa::path(
    post,
    path = "/api/v1/community/substitutions",
    tag = "communauté",
    request_body = SubstitutionInput,
    responses(
        (status = 201, body = Substitution),
        (status = 403, body = ErrorBody),
        (status = 409, description = "Existe déjà", body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn create_substitution(
    State(state): State<AppState>,
    caller: Caller,
    Json(input): Json<SubstitutionInput>,
) -> ApiResult<(StatusCode, Json<Substitution>)> {
    let admin = caller.require_admin(&state).await?;
    let note = check(input.factor, input.note.as_deref())?;
    let mut tx = state.db.begin().await?;
    let ids: Vec<(String, Uuid)> =
        sqlx::query_as("SELECT code, id FROM ingredients WHERE code = ANY($1)")
            .bind([input.ingredient.as_str(), input.substitute.as_str()])
            .fetch_all(&mut *tx)
            .await?;
    let id_of = |code: &str| ids.iter().find(|(c, _)| c == code).map(|(_, id)| *id);
    let (Some(ingredient), Some(substitute)) = (id_of(&input.ingredient), id_of(&input.substitute))
    else {
        return Err(ApiError::invalid("Deux ingrédients du référentiel."));
    };
    if ingredient == substitute {
        return Err(ApiError::invalid(
            "Un ingrédient ne se remplace pas par lui-même.",
        ));
    }
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO substitutions (id, ingredient_id, substitute_id, substitute_state, factor, note, source, feedback_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(ingredient)
    .bind(substitute)
    .bind(input.substitute_state.map(IngredientState::as_str))
    .bind(input.factor)
    .bind(&note)
    .bind(if input.feedback_id.is_some() { "feedback" } else { "admin" })
    .bind(input.feedback_id)
    .execute(&mut *tx)
    .await?;
    if let Some(feedback) = input.feedback_id {
        let row: Option<(Uuid, Option<Uuid>, String)> = sqlx::query_as(
            "UPDATE recipe_feedback f SET status = 'applied', reviewed_by = $2, reviewed_at = now(), updated_at = now()
             FROM recipes r
             WHERE f.id = $1 AND r.id = f.recipe_id AND f.kind = 'tried' AND f.visibility = 'community'
               AND f.status IN ('pending', 'published')
             RETURNING f.recipe_id, f.author_id, r.title",
        )
        .bind(feedback)
        .bind(admin)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((recipe_id, author, title)) = row else {
            return Err(ApiError::invalid(
                "Ce retour n'est pas une variante tentée en attente.",
            ));
        };
        let text = format!(
            "« {title} » a été amélioré avec ton retour : ta variante rejoint les remplacements."
        );
        notifications::notify(&mut tx, author, "feedback_applied", Some(recipe_id), &text).await?;
    }
    let created = fetch(&mut tx, id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Le coefficient et la note d'un remplacement.
#[derive(Deserialize, ToSchema)]
pub struct SubstitutionUpdate {
    pub factor: f32,
    pub note: Option<String>,
}

#[utoipa::path(
    put,
    path = "/api/v1/community/substitutions/{substitution_id}",
    tag = "communauté",
    params(("substitution_id" = Uuid, Path)),
    request_body = SubstitutionUpdate,
    responses(
        (status = 200, body = Substitution),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn update_substitution(
    State(state): State<AppState>,
    caller: Caller,
    Path(substitution_id): Path<Uuid>,
    Json(input): Json<SubstitutionUpdate>,
) -> ApiResult<Json<Substitution>> {
    caller.require_admin(&state).await?;
    let note = check(input.factor, input.note.as_deref())?;
    let mut conn = state.db.acquire().await?;
    sqlx::query("UPDATE substitutions SET factor = $2, note = $3 WHERE id = $1")
        .bind(substitution_id)
        .bind(input.factor)
        .bind(&note)
        .execute(&mut *conn)
        .await?;
    fetch(&mut conn, substitution_id).await.map(Json)
}

/// Supprime un remplacement (admin) ; les lignes de courses qui l'avaient choisi reviennent à l'ingrédient.
#[utoipa::path(
    delete,
    path = "/api/v1/community/substitutions/{substitution_id}",
    tag = "communauté",
    params(("substitution_id" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn delete_substitution(
    State(state): State<AppState>,
    caller: Caller,
    Path(substitution_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    caller.require_admin(&state).await?;
    let deleted = sqlx::query("DELETE FROM substitutions WHERE id = $1")
        .bind(substitution_id)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Ce que la recette dit d'un remplacement.
#[derive(Deserialize, ToSchema)]
pub struct RuleInput {
    pub allowed: bool,
    #[serde(default)]
    pub note: String,
}

/// La recette de ce slug, que l'appelant peut modifier (du foyer ; de la communauté : l'admin).
async fn editable(
    state: &AppState,
    caller: &Caller,
    conn: &mut PgConnection,
    household_id: Uuid,
    slug: &str,
) -> ApiResult<Uuid> {
    caller
        .require(&state.db, household_id, Scope::RecipesWrite)
        .await?;
    let (id, owner) = recipes::find(conn, household_id, slug)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if owner.is_none() {
        caller.require_admin(state).await?;
    }
    Ok(id)
}

/// Précise ou refuse un remplacement dans une recette (ceux qui la modifient).
#[utoipa::path(
    put,
    path = "/api/v1/households/{id}/recipes/{slug}/substitutions/{substitution_id}",
    tag = "recettes",
    params(("id" = Uuid, Path), ("slug" = String, Path), ("substitution_id" = Uuid, Path)),
    request_body = RuleInput,
    responses(
        (status = 200, body = Recipe),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn set_rule(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, slug, substitution_id)): Path<(Uuid, String, Uuid)>,
    Json(input): Json<RuleInput>,
) -> ApiResult<Json<Recipe>> {
    let mut conn = state.db.acquire().await?;
    let recipe_id = editable(&state, &caller, &mut conn, id, &slug).await?;
    let note = check(1.0, Some(&input.note))?;
    let applies: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM substitutions s JOIN recipe_ingredients l ON l.ingredient_id = s.ingredient_id
                        WHERE s.id = $1 AND l.recipe_id = $2)",
    )
    .bind(substitution_id)
    .bind(recipe_id)
    .fetch_one(&mut *conn)
    .await?;
    if !applies {
        return Err(ApiError::invalid(
            "Ce remplacement ne concerne aucun ingrédient de la recette.",
        ));
    }
    sqlx::query(
        "INSERT INTO recipe_substitutions (recipe_id, substitution_id, allowed, note) VALUES ($1, $2, $3, $4)
         ON CONFLICT (recipe_id, substitution_id) DO UPDATE SET allowed = EXCLUDED.allowed, note = EXCLUDED.note",
    )
    .bind(recipe_id)
    .bind(substitution_id)
    .bind(input.allowed)
    .bind(&note)
    .execute(&mut *conn)
    .await?;
    Ok(Json(recipes::load_by_id(&state.db, id, recipe_id).await?))
}

/// La recette revient à ce que dit le remplacement.
#[utoipa::path(
    delete,
    path = "/api/v1/households/{id}/recipes/{slug}/substitutions/{substitution_id}",
    tag = "recettes",
    params(("id" = Uuid, Path), ("slug" = String, Path), ("substitution_id" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn delete_rule(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, slug, substitution_id)): Path<(Uuid, String, Uuid)>,
) -> ApiResult<StatusCode> {
    let mut conn = state.db.acquire().await?;
    let recipe_id = editable(&state, &caller, &mut conn, id, &slug).await?;
    sqlx::query("DELETE FROM recipe_substitutions WHERE recipe_id = $1 AND substitution_id = $2")
        .bind(recipe_id)
        .bind(substitution_id)
        .execute(&mut *conn)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
