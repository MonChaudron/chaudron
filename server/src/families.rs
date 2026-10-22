//! Familles de recettes : les déclinaisons d'un même plat, et la préférée de chaque foyer. Les recettes proches d'un
//! brouillon, pour demander « c'en est une version ? » : à part, une autre déclinaison, ou la même en mieux.

use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{post, put},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::Caller,
    error::{ApiError, ApiResult, ErrorBody},
    recipes::{self, Recipe, RecipeInput},
    similar::{self, Profile},
    text::search_key,
    types::{PortionRole, Scope},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/households/{id}/recipes/similar", post(similar_recipes))
        .route(
            "/households/{id}/recipes/{slug}/family",
            put(join_family).delete(leave_family),
        )
        .route(
            "/households/{id}/families/{family_id}/preferred",
            put(prefer),
        )
}

/// La famille d'une recette, vue par un foyer.
#[derive(Serialize, ToSchema)]
pub struct Family {
    pub id: Uuid,
    pub title: String,
    /// Les déclinaisons que le foyer voit (la recette comprise).
    pub members: Vec<FamilyMember>,
    /// Slug de la préférée du foyer.
    pub preferred: Option<String>,
}

#[derive(Serialize, ToSchema, FromRow)]
pub struct FamilyMember {
    pub slug: String,
    pub title: String,
    pub community: bool,
}

/// La famille, vue par ce foyer (sans foyer : les recettes de la communauté).
pub(crate) async fn family(
    conn: &mut PgConnection,
    family_id: Uuid,
    viewer: Option<Uuid>,
) -> ApiResult<Family> {
    let title: String = sqlx::query_scalar("SELECT title FROM recipe_families WHERE id = $1")
        .bind(family_id)
        .fetch_one(&mut *conn)
        .await?;
    let members = sqlx::query_as(
        "SELECT slug, title, household_id IS NULL AS community FROM recipes
         WHERE family_id = $1 AND (household_id = $2 OR (household_id IS NULL AND retired_at IS NULL))
         ORDER BY search_key",
    )
    .bind(family_id)
    .bind(viewer)
    .fetch_all(&mut *conn)
    .await?;
    let preferred = sqlx::query_scalar(
        "SELECT r.slug FROM family_preferences p JOIN recipes r ON r.id = p.recipe_id
         WHERE p.household_id = $1 AND p.family_id = $2",
    )
    .bind(viewer)
    .bind(family_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(Family {
        id: family_id,
        title,
        members,
        preferred,
    })
}

/// Une recette proche d'un brouillon.
#[derive(Serialize, ToSchema, Clone)]
pub struct Similar {
    pub slug: String,
    pub title: String,
    pub community: bool,
    /// De 0 à 100.
    pub score: i64,
    /// Pourquoi : « Pomme de terre, Ail, Crème en commun ».
    pub reasons: Vec<String>,
}

/// Les recettes que voit le foyer, proches de ce brouillon : trois au plus, de la plus proche à la moins proche.
pub(crate) async fn similar_to(
    conn: &mut PgConnection,
    household_id: Uuid,
    draft: &RecipeInput,
    exclude: Option<&str>,
) -> ApiResult<Vec<Similar>> {
    type Candidate = (Uuid, String, String, bool);
    let candidates: Vec<Candidate> = sqlx::query_as(
        "SELECT id, slug, title, household_id IS NULL FROM recipes
         WHERE (household_id = $1 OR (household_id IS NULL AND retired_at IS NULL))",
    )
    .bind(household_id)
    .fetch_all(&mut *conn)
    .await?;
    let ids: Vec<Uuid> = candidates.iter().map(|c| c.0).collect();
    let lines: Vec<(Uuid, String, String, String)> = sqlx::query_as(
        "SELECT l.recipe_id, i.code, i.name, l.role FROM recipe_ingredients l
         JOIN ingredients i ON i.id = l.ingredient_id WHERE l.recipe_id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(&mut *conn)
    .await?;
    let equipment: Vec<(Uuid, Vec<String>)> = sqlx::query_as(
        "SELECT recipe_id, equipment_codes FROM recipe_variants WHERE recipe_id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(&mut *conn)
    .await?;
    let mut names: HashMap<String, String> = HashMap::new();
    let mut by_recipe: HashMap<Uuid, Vec<(String, PortionRole)>> = HashMap::new();
    for (recipe, code, name, role) in lines {
        if let Ok(role) = PortionRole::try_from(role) {
            by_recipe
                .entry(recipe)
                .or_default()
                .push((code.clone(), role));
        }
        names.insert(code, name);
    }
    let mut tools: HashMap<Uuid, Vec<String>> = HashMap::new();
    for (recipe, codes) in equipment {
        tools.entry(recipe).or_default().extend(codes);
    }
    let mine = Profile::new(
        &draft.title,
        draft
            .ingredients
            .iter()
            .map(|l| (l.ingredient.as_str(), l.role)),
        draft
            .variants
            .iter()
            .flat_map(|v| v.equipment.iter().map(String::as_str)),
    );
    let mut found: Vec<Similar> = Vec::new();
    for (id, slug, title, community) in candidates {
        if exclude == Some(slug.as_str()) {
            continue;
        }
        let empty = Vec::new();
        let theirs = Profile::new(
            &title,
            by_recipe
                .get(&id)
                .unwrap_or(&empty)
                .iter()
                .map(|(c, r)| (c.as_str(), *r)),
            tools
                .get(&id)
                .unwrap_or(&Vec::new())
                .iter()
                .map(String::as_str),
        );
        let (score, common) = similar::score(&mine, &theirs);
        if score < similar::THRESHOLD {
            continue;
        }
        let common: Vec<&str> = common
            .iter()
            .take(4)
            .map(|c| names.get(c).map_or(c.as_str(), String::as_str))
            .collect();
        found.push(Similar {
            slug,
            title,
            community,
            score: (score * 100.0).round() as i64,
            reasons: if common.is_empty() {
                Vec::new()
            } else {
                vec![format!("{} en commun", common.join(", "))]
            },
        });
    }
    found.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| search_key(&a.title).cmp(&search_key(&b.title)))
    });
    found.truncate(3);
    Ok(found)
}

/// Les recettes proches d'un brouillon (avant de le publier, ou de le proposer à la communauté). Rien n'est fait :
/// l'auteur décide.
#[utoipa::path(
    post,
    path = "/api/v1/households/{id}/recipes/similar",
    tag = "recettes",
    params(("id" = Uuid, Path)),
    request_body = RecipeInput,
    responses(
        (status = 200, body = Vec<Similar>),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn similar_recipes(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Json(draft): Json<RecipeInput>,
) -> ApiResult<Json<Vec<Similar>>> {
    caller.require(&state.db, id, Scope::Read).await?;
    let mut conn = state.db.acquire().await?;
    similar_to(&mut conn, id, &draft, draft.slug.as_deref())
        .await
        .map(Json)
}

/// Range deux recettes dans la même famille (créée au besoin, au nom de la seconde).
pub(crate) async fn join(conn: &mut PgConnection, recipe_id: Uuid, with: Uuid) -> ApiResult<Uuid> {
    let (family, title): (Option<Uuid>, String) =
        sqlx::query_as("SELECT family_id, title FROM recipes WHERE id = $1")
            .bind(with)
            .fetch_one(&mut *conn)
            .await?;
    let family = match family {
        Some(family) => family,
        None => {
            let family = Uuid::now_v7();
            sqlx::query("INSERT INTO recipe_families (id, title) VALUES ($1, $2)")
                .bind(family)
                .bind(&title)
                .execute(&mut *conn)
                .await?;
            sqlx::query("UPDATE recipes SET family_id = $2 WHERE id = $1")
                .bind(with)
                .bind(family)
                .execute(&mut *conn)
                .await?;
            family
        }
    };
    sqlx::query("UPDATE recipes SET family_id = $2 WHERE id = $1")
        .bind(recipe_id)
        .bind(family)
        .execute(&mut *conn)
        .await?;
    Ok(family)
}

/// Une autre déclinaison de quelle recette.
#[derive(Deserialize, ToSchema)]
pub struct JoinInput {
    /// Slug de la recette dont celle-ci est une déclinaison.
    pub with: String,
}

/// La recette de ce slug, rangeable par l'appelant : une recette du foyer, ou de la communauté pour l'admin.
async fn arrangeable(
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

/// Range la recette avec ses déclinaisons. Une recette de la communauté : l'admin seulement.
#[utoipa::path(
    put,
    path = "/api/v1/households/{id}/recipes/{slug}/family",
    tag = "recettes",
    params(("id" = Uuid, Path), ("slug" = String, Path)),
    request_body = JoinInput,
    responses(
        (status = 200, body = Recipe),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn join_family(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, slug)): Path<(Uuid, String)>,
    Json(input): Json<JoinInput>,
) -> ApiResult<Json<Recipe>> {
    let mut tx = state.db.begin().await?;
    let recipe_id = arrangeable(&state, &caller, &mut tx, id, &slug).await?;
    let (with, _) = recipes::find(&mut tx, id, &input.with)
        .await?
        .ok_or_else(|| ApiError::invalid(format!("Recette inconnue : {}.", input.with)))?;
    if with == recipe_id {
        return Err(ApiError::invalid(
            "Une recette n'est pas sa propre déclinaison.",
        ));
    }
    join(&mut tx, recipe_id, with).await?;
    tx.commit().await?;
    Ok(Json(recipes::load_by_id(&state.db, id, recipe_id).await?))
}

/// Sort la recette de sa famille.
#[utoipa::path(
    delete,
    path = "/api/v1/households/{id}/recipes/{slug}/family",
    tag = "recettes",
    params(("id" = Uuid, Path), ("slug" = String, Path)),
    responses(
        (status = 204),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn leave_family(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, slug)): Path<(Uuid, String)>,
) -> ApiResult<StatusCode> {
    let mut tx = state.db.begin().await?;
    let recipe_id = arrangeable(&state, &caller, &mut tx, id, &slug).await?;
    sqlx::query("UPDATE recipes SET family_id = NULL WHERE id = $1")
        .bind(recipe_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM family_preferences WHERE recipe_id = $1")
        .bind(recipe_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// La préférée.
#[derive(Deserialize, ToSchema)]
pub struct PreferInput {
    pub slug: String,
}

/// La déclinaison préférée du foyer dans cette famille : proposée en premier (catalogue, composition du menu).
#[utoipa::path(
    put,
    path = "/api/v1/households/{id}/families/{family_id}/preferred",
    tag = "recettes",
    params(("id" = Uuid, Path), ("family_id" = Uuid, Path)),
    request_body = PreferInput,
    responses(
        (status = 204),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn prefer(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, family_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<PreferInput>,
) -> ApiResult<StatusCode> {
    caller.require(&state.db, id, Scope::RecipesWrite).await?;
    let mut conn = state.db.acquire().await?;
    let recipe_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM recipes
         WHERE slug = $1 AND family_id = $2 AND (household_id = $3 OR household_id IS NULL)",
    )
    .bind(&input.slug)
    .bind(family_id)
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(ApiError::not_found)?;
    sqlx::query(
        "INSERT INTO family_preferences (household_id, family_id, recipe_id) VALUES ($1, $2, $3)
         ON CONFLICT (household_id, family_id) DO UPDATE SET recipe_id = EXCLUDED.recipe_id",
    )
    .bind(id)
    .bind(family_id)
    .bind(recipe_id)
    .execute(&mut *conn)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
