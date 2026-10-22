//! La communauté de l'instance : un foyer propose une de ses recettes, l'admin l'accepte (elle devient la recette de
//! la communauté, même identifiant : les menus du foyer ne bougent pas) ou la refuse avec un mot ; le foyer est
//! prévenu. Un foyer copie une recette de la communauté chez lui ; la copie garde les clés de l'originale, ce qui
//! permet de voir ce qui a changé depuis et d'en reprendre, élément par élément.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgConnection};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::Caller,
    error::{ApiError, ApiResult, ErrorBody},
    notifications,
    recipes::{self, Recipe, RecipeInput},
    types::{Scope, text_enum},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/households/{id}/recipes/{slug}/submit",
            post(submit_recipe),
        )
        .route("/households/{id}/recipes/{slug}/copy", post(copy_recipe))
        .route(
            "/households/{id}/recipes/{slug}/upstream/take",
            post(take_upstream),
        )
        .route("/community/review", get(review_queue))
        .route(
            "/community/submissions/{submission_id}",
            get(read_submission),
        )
        .route(
            "/community/submissions/{submission_id}/accept",
            post(accept_submission),
        )
        .route(
            "/community/submissions/{submission_id}/refuse",
            post(refuse_submission),
        )
}

text_enum! {
    /// Où en est une recette proposée.
    pub enum SubmissionStatus {
        Pending = "pending",
        Accepted = "accepted",
        Refused = "refused",
    }
}

/// Une recette proposée à la communauté.
#[derive(Serialize, ToSchema, FromRow)]
pub struct Submission {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub household_name: Option<String>,
    pub author_name: String,
    pub message: String,
    #[sqlx(try_from = "String")]
    pub status: SubmissionStatus,
    pub review_message: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

const SUBMISSION: &str =
    "SELECT s.id, r.slug, r.title, h.name AS household_name, s.author_name, s.message, s.status,
                                 s.review_message, s.created_at
                          FROM recipe_submissions s JOIN recipes r ON r.id = s.recipe_id
                          LEFT JOIN households h ON h.id = s.household_id";

async fn submission(conn: &mut PgConnection, id: Uuid) -> ApiResult<Submission> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("{SUBMISSION} WHERE s.id = $1")))
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(ApiError::not_found)
}

fn message(text: Option<&str>) -> ApiResult<String> {
    let text = text.unwrap_or_default().trim();
    if text.chars().count() > 2000 {
        return Err(ApiError::invalid("Le mot fait 2 000 caractères au plus."));
    }
    Ok(text.to_owned())
}

/// Un mot pour l'admin.
#[derive(Deserialize, ToSchema)]
pub struct SubmitInput {
    pub message: Option<String>,
}

/// Propose une recette du foyer à la communauté (tout compte du foyer). Une seule proposition en attente par recette ;
/// une copie ne se propose pas (ses changements se proposent à l'originale).
#[utoipa::path(
    post,
    path = "/api/v1/households/{id}/recipes/{slug}/submit",
    tag = "communauté",
    params(("id" = Uuid, Path), ("slug" = String, Path)),
    request_body = SubmitInput,
    responses(
        (status = 201, body = Submission),
        (status = 404, body = ErrorBody),
        (status = 409, description = "Déjà proposée", body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn submit_recipe(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, slug)): Path<(Uuid, String)>,
    Json(input): Json<SubmitInput>,
) -> ApiResult<(StatusCode, Json<Submission>)> {
    caller.require(&state.db, id, Scope::RecipesWrite).await?;
    let author = caller.author(&state.db).await?;
    let mut tx = state.db.begin().await?;
    let (recipe_id, owner) = recipes::find(&mut tx, id, &slug)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if owner.is_none() {
        return Err(ApiError::invalid(
            "Cette recette est déjà dans la communauté.",
        ));
    }
    let copy: bool =
        sqlx::query_scalar("SELECT copied_from IS NOT NULL FROM recipes WHERE id = $1")
            .bind(recipe_id)
            .fetch_one(&mut *tx)
            .await?;
    if copy {
        return Err(ApiError::invalid(
            "C'est une copie d'une recette de la communauté : propose tes changements à l'originale.",
        ));
    }
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM recipe_submissions WHERE recipe_id = $1 AND status = 'pending')",
    )
    .bind(recipe_id)
    .fetch_one(&mut *tx)
    .await?;
    if pending {
        return Err(ApiError::conflict(
            "already_submitted",
            "Cette recette attend déjà la réponse de l'admin.",
        ));
    }
    let submission_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO recipe_submissions (id, recipe_id, household_id, author_id, author_name, message, status)
         VALUES ($1, $2, $3, $4, $5, $6, 'pending')",
    )
    .bind(submission_id)
    .bind(recipe_id)
    .bind(id)
    .bind(author.id)
    .bind(&author.name)
    .bind(message(input.message.as_deref())?)
    .execute(&mut *tx)
    .await?;
    let created = submission(&mut tx, submission_id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Ce qui attend l'admin.
#[derive(Serialize, ToSchema)]
pub struct Review {
    /// Recettes proposées, de la plus ancienne à la plus récente.
    pub submissions: Vec<Submission>,
}

/// « À revoir » (admin de l'instance).
#[utoipa::path(
    get,
    path = "/api/v1/community/review",
    tag = "communauté",
    responses(
        (status = 200, body = Review),
        (status = 403, body = ErrorBody),
    )
)]
pub async fn review_queue(
    State(state): State<AppState>,
    caller: Caller,
) -> ApiResult<Json<Review>> {
    caller.require_admin(&state).await?;
    let submissions = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SUBMISSION} WHERE s.status = 'pending' ORDER BY s.created_at"
    )))
    .fetch_all(&state.db)
    .await?;
    Ok(Json(Review { submissions }))
}

/// Une proposition et la recette telle qu'elle est proposée.
#[derive(Serialize, ToSchema)]
pub struct SubmissionDetail {
    pub submission: Submission,
    pub recipe: Recipe,
}

#[utoipa::path(
    get,
    path = "/api/v1/community/submissions/{submission_id}",
    tag = "communauté",
    params(("submission_id" = Uuid, Path)),
    responses(
        (status = 200, body = SubmissionDetail),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn read_submission(
    State(state): State<AppState>,
    caller: Caller,
    Path(submission_id): Path<Uuid>,
) -> ApiResult<Json<SubmissionDetail>> {
    caller.require_admin(&state).await?;
    let mut conn = state.db.acquire().await?;
    let submission = submission(&mut conn, submission_id).await?;
    let recipe_id: Uuid =
        sqlx::query_scalar("SELECT recipe_id FROM recipe_submissions WHERE id = $1")
            .bind(submission_id)
            .fetch_one(&mut *conn)
            .await?;
    let recipe = recipes::load_plain(&mut conn, recipe_id).await?;
    Ok(Json(SubmissionDetail { submission, recipe }))
}

/// Accepter : avec un autre slug, et la recette modifiée (« Modifier puis accepter »), si besoin.
#[derive(Deserialize, ToSchema)]
pub struct AcceptInput {
    /// Le slug dans la communauté ; par défaut, celui du foyer.
    pub slug: Option<String>,
    /// La recette telle que l'admin la publie.
    pub recipe: Option<RecipeInput>,
    /// Un mot pour le foyer.
    pub message: Option<String>,
}

/// La proposition en attente, verrouillée : (recette, auteur, titre).
async fn pending(
    conn: &mut PgConnection,
    submission_id: Uuid,
) -> ApiResult<(Uuid, Option<Uuid>, String)> {
    let row: (Uuid, Option<Uuid>, String, String) = sqlx::query_as(
        "SELECT s.recipe_id, s.author_id, s.status, r.title FROM recipe_submissions s
         JOIN recipes r ON r.id = s.recipe_id WHERE s.id = $1 FOR UPDATE OF s",
    )
    .bind(submission_id)
    .fetch_optional(conn)
    .await?
    .ok_or_else(ApiError::not_found)?;
    if row.2 != "pending" {
        return Err(ApiError::conflict(
            "already_reviewed",
            "Cette proposition a déjà une réponse.",
        ));
    }
    Ok((row.0, row.1, row.3))
}

/// Accepte une recette proposée (admin) : elle devient la recette de la communauté. 409 `slug_taken` si une autre
/// recette (d'un foyer ou de la communauté) a ce slug.
#[utoipa::path(
    post,
    path = "/api/v1/community/submissions/{submission_id}/accept",
    tag = "communauté",
    params(("submission_id" = Uuid, Path)),
    request_body = AcceptInput,
    responses(
        (status = 200, body = Recipe),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn accept_submission(
    State(state): State<AppState>,
    caller: Caller,
    Path(submission_id): Path<Uuid>,
    Json(input): Json<AcceptInput>,
) -> ApiResult<Json<Recipe>> {
    let admin = caller.require_admin(&state).await?;
    let author = caller.author(&state.db).await?;
    let mut tx = state.db.begin().await?;
    let (recipe_id, submitter, title) = pending(&mut tx, submission_id).await?;
    let current: String = sqlx::query_scalar("SELECT slug FROM recipes WHERE id = $1")
        .bind(recipe_id)
        .fetch_one(&mut *tx)
        .await?;
    let slug = input
        .slug
        .as_deref()
        .map(str::trim)
        .unwrap_or(&current)
        .to_owned();
    recipes::check_slug(&slug)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('recipe:' || $1))")
        .bind(&slug)
        .execute(&mut *tx)
        .await?;
    let taken: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM recipes WHERE slug = $1 AND id <> $2)")
            .bind(&slug)
            .bind(recipe_id)
            .fetch_one(&mut *tx)
            .await?;
    if taken {
        return Err(ApiError::conflict(
            "slug_taken",
            format!("« {slug} » est déjà le slug d'une autre recette : choisis-en un autre."),
        ));
    }
    sqlx::query(
        "UPDATE recipes SET household_id = NULL, slug = $2, updated_at = now() WHERE id = $1",
    )
    .bind(recipe_id)
    .bind(&slug)
    .execute(&mut *tx)
    .await?;
    match input.recipe {
        Some(mut edited) => {
            edited.slug = None;
            edited.message = Some("Acceptée dans la communauté".into());
            recipes::save(&mut tx, None, &slug, &edited, &author).await?;
        }
        None => {
            recipes::record_version(
                &mut tx,
                recipe_id,
                &author,
                "Acceptée dans la communauté",
                true,
            )
            .await?;
        }
    }
    let word = message(input.message.as_deref())?;
    sqlx::query(
        "UPDATE recipe_submissions SET status = 'accepted', review_message = nullif($2, ''), reviewed_by = $3,
                reviewed_at = now()
         WHERE id = $1",
    )
    .bind(submission_id)
    .bind(&word)
    .bind(admin)
    .execute(&mut *tx)
    .await?;
    let text = if word.is_empty() {
        format!("« {title} » a rejoint la communauté.")
    } else {
        format!("« {title} » a rejoint la communauté : {word}")
    };
    notifications::notify(
        &mut tx,
        submitter,
        "submission_accepted",
        Some(recipe_id),
        &text,
    )
    .await?;
    let recipe = recipes::load_plain(&mut tx, recipe_id).await?;
    tx.commit().await?;
    Ok(Json(recipe))
}

/// Refuser, avec un mot.
#[derive(Deserialize, ToSchema)]
pub struct RefuseInput {
    pub message: String,
}

/// Refuse une recette proposée (admin), avec un mot pour le foyer.
#[utoipa::path(
    post,
    path = "/api/v1/community/submissions/{submission_id}/refuse",
    tag = "communauté",
    params(("submission_id" = Uuid, Path)),
    request_body = RefuseInput,
    responses(
        (status = 204),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn refuse_submission(
    State(state): State<AppState>,
    caller: Caller,
    Path(submission_id): Path<Uuid>,
    Json(input): Json<RefuseInput>,
) -> ApiResult<StatusCode> {
    let admin = caller.require_admin(&state).await?;
    let word = message(Some(&input.message))?;
    if word.is_empty() {
        return Err(ApiError::invalid(
            "Un mot pour le foyer : pourquoi pas cette fois.",
        ));
    }
    let mut tx = state.db.begin().await?;
    let (recipe_id, submitter, title) = pending(&mut tx, submission_id).await?;
    sqlx::query(
        "UPDATE recipe_submissions SET status = 'refused', review_message = $2, reviewed_by = $3, reviewed_at = now()
         WHERE id = $1",
    )
    .bind(submission_id)
    .bind(&word)
    .bind(admin)
    .execute(&mut *tx)
    .await?;
    let text = format!("« {title} » n'a pas rejoint la communauté : {word}");
    notifications::notify(
        &mut tx,
        submitter,
        "submission_refused",
        Some(recipe_id),
        &text,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// La copie : son slug ; par défaut, celui de l'originale suivi de `-maison`.
#[derive(Deserialize, ToSchema)]
pub struct CopyInput {
    pub slug: Option<String>,
}

/// Copie une recette de la communauté dans le foyer : une recette du foyer, modifiable librement, qui garde le lien
/// avec l'originale et ses clés. Les menus qui utilisent l'originale ne changent pas.
#[utoipa::path(
    post,
    path = "/api/v1/households/{id}/recipes/{slug}/copy",
    tag = "communauté",
    params(("id" = Uuid, Path), ("slug" = String, Path)),
    request_body = CopyInput,
    responses(
        (status = 201, body = Recipe),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn copy_recipe(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, slug)): Path<(Uuid, String)>,
    Json(input): Json<CopyInput>,
) -> ApiResult<(StatusCode, Json<Recipe>)> {
    caller.require(&state.db, id, Scope::RecipesWrite).await?;
    let author = caller.author(&state.db).await?;
    let mut tx = state.db.begin().await?;
    let (original_id, owner) = recipes::find(&mut tx, id, &slug)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if owner.is_some() {
        return Err(ApiError::invalid(
            "On copie une recette de la communauté ; celle-ci est déjà au foyer.",
        ));
    }
    let original = recipes::load_plain(&mut tx, original_id).await?;
    let target = input.slug.unwrap_or_else(|| format!("{slug}-maison"));
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM recipes WHERE slug = $1 AND (household_id = $2 OR household_id IS NULL))",
    )
    .bind(&target)
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if exists {
        return Err(ApiError::conflict(
            "slug_taken",
            format!("Le foyer voit déjà une recette « {target} » : choisis un autre slug."),
        ));
    }
    let mut content = original.to_input();
    content.slug = None;
    content.message = Some(format!(
        "Copie de « {} », v{}",
        original.title, original.version
    ));
    let saved = recipes::save(&mut tx, Some(id), &target, &content, &author).await?;
    sqlx::query("UPDATE recipes SET copied_from = $2, copied_version = $3 WHERE id = $1")
        .bind(saved.id)
        .bind(original_id)
        .bind(original.version)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(recipes::load_by_id(&state.db, id, saved.id).await?),
    ))
}

/// Reprendre des changements de l'originale.
#[derive(Deserialize, ToSchema)]
pub struct TakeInput {
    /// Clés des éléments de l'originale à reprendre (ajoutés, changés ; retirés de l'originale : retirés de la
    /// copie).
    #[serde(default)]
    pub keys: Vec<Uuid>,
    /// Le reste est vu : la copie suit désormais la version courante de l'originale.
    #[serde(default)]
    pub seen: bool,
}

/// Où se trouve un élément dans une recette (en JSON, comme on la publie).
enum Place {
    Line(usize),
    Variant(usize),
    Phase(usize, usize),
    Step(usize, usize, usize),
}

fn key_of(item: &Value) -> Option<&str> {
    item.get("key").and_then(Value::as_str)
}

fn list(value: &Value, field: &str) -> Vec<Value> {
    value
        .get(field)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn locate(recipe: &Value, key: &str) -> Option<Place> {
    if let Some(i) = list(recipe, "ingredients")
        .iter()
        .position(|l| key_of(l) == Some(key))
    {
        return Some(Place::Line(i));
    }
    for (v, variant) in list(recipe, "variants").iter().enumerate() {
        if key_of(variant) == Some(key) {
            return Some(Place::Variant(v));
        }
        for (p, phase) in list(variant, "phases").iter().enumerate() {
            if key_of(phase) == Some(key) {
                return Some(Place::Phase(v, p));
            }
            if let Some(s) = list(phase, "steps")
                .iter()
                .position(|s| key_of(s) == Some(key))
            {
                return Some(Place::Step(v, p, s));
            }
        }
    }
    None
}

/// Le tableau d'un objet JSON, créé vide s'il manque.
fn array<'a>(value: &'a mut Value, field: &str) -> &'a mut Vec<Value> {
    if !value.get(field).is_some_and(Value::is_array) {
        value[field] = Value::Array(Vec::new());
    }
    value[field].as_array_mut().expect("tableau")
}

/// Remplace ou ajoute `item` (de même clé) dans `items`, à la place qu'il a dans l'originale ; `keep` : les champs de
/// la copie gardés (les enfants, repris un par un).
fn put(items: &mut Vec<Value>, item: &Value, index: usize, keep: Option<&str>) {
    let key = key_of(item).map(str::to_owned);
    match items.iter().position(|i| key_of(i) == key.as_deref()) {
        Some(at) => {
            let mut next = item.clone();
            if let Some(field) = keep {
                next[field] = items[at][field].clone();
            }
            items[at] = next;
        }
        None => items.insert(index.min(items.len()), item.clone()),
    }
}

/// Reprend un élément de l'originale dans la copie : le remplace, l'ajoute, ou le retire s'il n'existe plus.
fn take(copy: &mut Value, original: &Value, key: &str) {
    match locate(original, key) {
        Some(Place::Line(i)) => put(
            array(copy, "ingredients"),
            &list(original, "ingredients")[i],
            i,
            None,
        ),
        Some(Place::Variant(v)) => put(
            array(copy, "variants"),
            &list(original, "variants")[v],
            v,
            Some("phases"),
        ),
        Some(Place::Phase(v, p)) => {
            let variant = &list(original, "variants")[v];
            let parent = key_of(variant).unwrap_or_default().to_owned();
            if let Some(target) = array(copy, "variants")
                .iter_mut()
                .find(|c| key_of(c) == Some(&parent))
            {
                put(
                    array(target, "phases"),
                    &list(variant, "phases")[p],
                    p,
                    Some("steps"),
                );
            }
        }
        Some(Place::Step(v, p, s)) => {
            let phase = &list(&list(original, "variants")[v], "phases")[p];
            let parent = key_of(phase).unwrap_or_default().to_owned();
            for variant in array(copy, "variants").iter_mut() {
                if let Some(target) = array(variant, "phases")
                    .iter_mut()
                    .find(|c| key_of(c) == Some(&parent))
                {
                    put(array(target, "steps"), &list(phase, "steps")[s], s, None);
                }
            }
        }
        // Retiré de l'originale : retiré de la copie.
        None => match locate(copy, key) {
            Some(Place::Line(i)) => {
                array(copy, "ingredients").remove(i);
            }
            Some(Place::Variant(v)) => {
                array(copy, "variants").remove(v);
            }
            Some(Place::Phase(v, p)) => {
                array(&mut array(copy, "variants")[v], "phases").remove(p);
            }
            Some(Place::Step(v, p, s)) => {
                let variant = &mut array(copy, "variants")[v];
                array(&mut array(variant, "phases")[p], "steps").remove(s);
            }
            None => {}
        },
    }
}

/// Reprend dans la copie des changements de l'originale, élément par élément (il remplace celui de la copie) ; pas
/// de fusion à trois versions. `seen` : la copie suit la version courante de l'originale.
#[utoipa::path(
    post,
    path = "/api/v1/households/{id}/recipes/{slug}/upstream/take",
    tag = "communauté",
    params(("id" = Uuid, Path), ("slug" = String, Path)),
    request_body = TakeInput,
    responses(
        (status = 200, body = Recipe),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn take_upstream(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, slug)): Path<(Uuid, String)>,
    Json(input): Json<TakeInput>,
) -> ApiResult<Json<Recipe>> {
    caller.require(&state.db, id, Scope::RecipesWrite).await?;
    let author = caller.author(&state.db).await?;
    let mut tx = state.db.begin().await?;
    let (copy_id, owner) = recipes::find(&mut tx, id, &slug)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let original_id: Option<Uuid> =
        sqlx::query_scalar("SELECT copied_from FROM recipes WHERE id = $1")
            .bind(copy_id)
            .fetch_one(&mut *tx)
            .await?;
    let (Some(_), Some(original_id)) = (owner, original_id) else {
        return Err(ApiError::invalid(
            "Ce n'est pas une copie d'une recette de la communauté.",
        ));
    };
    let original = recipes::load_plain(&mut tx, original_id).await?;
    if !input.keys.is_empty() {
        let source = serde_json::to_value(original.to_input()).map_err(|_| ApiError::internal())?;
        let mine = recipes::load_plain(&mut tx, copy_id).await?;
        let mut target = serde_json::to_value(mine.to_input()).map_err(|_| ApiError::internal())?;
        for key in &input.keys {
            take(&mut target, &source, &key.to_string());
        }
        let mut content: RecipeInput = serde_json::from_value(target)
            .map_err(|e| ApiError::invalid(format!("Reprise impossible : {e}.")))?;
        // Une seule variante par défaut : la première qui l'est.
        let mut found = false;
        for variant in &mut content.variants {
            if variant.is_default {
                variant.is_default = !found;
                found = true;
            }
        }
        content.slug = None;
        content.message = Some(format!(
            "Repris de « {} », v{}",
            original.title, original.version
        ));
        recipes::save(&mut tx, Some(id), &slug, &content, &author).await?;
    }
    if input.seen {
        sqlx::query("UPDATE recipes SET copied_version = $2 WHERE id = $1")
            .bind(copy_id)
            .bind(original.version)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(recipes::load_by_id(&state.db, id, copy_id).await?))
}
