//! Retours sur une recette. Le cahier du foyer : des notes privées, sur la recette entière ou sur une étape (par sa
//! clé), sur n'importe quelle recette que le foyer voit ; elles ne passent jamais en revue. Une note peut porter un
//! changement (« chez nous : 14 min »), affiché à côté du texte de l'étape sans toucher la recette.
//! Et « comment c'était ? » : une note par foyer et par repas.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, patch, put},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sqlx::{FromRow, PgConnection};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{Author, Caller},
    error::{ApiError, ApiResult, ErrorBody},
    menus::Day,
    recipes::{self, Element, Recipe},
    types::{Meal, Scope, text_enum},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/households/{id}/recipes/{slug}/feedback",
            get(list_feedback).post(create_feedback),
        )
        .route(
            "/households/{id}/feedback/{feedback_id}",
            patch(update_feedback).delete(delete_feedback),
        )
        .route("/households/{id}/ratings", put(rate_meal))
}

text_enum! {
    /// Sorte de retour.
    pub enum FeedbackKind {
        /// Un texte libre (une note du cahier, un commentaire).
        Comment = "comment",
        /// Une proposition de changement sur une étape (communauté).
        Proposal = "proposal",
        /// Une variante tentée : un ingrédient remplacé, et si c'était bien.
        Tried = "tried",
    }
}

text_enum! {
    /// Qui voit le retour.
    pub enum Visibility {
        /// Le foyer de l'auteur seulement (le cahier du foyer).
        Household = "household",
        /// Tous ceux qui voient la recette, une fois validé par l'admin.
        Community = "community",
    }
}

text_enum! {
    /// Où en est le retour. Cahier du foyer : `open`, `closed`. Communauté : `pending` (vu du foyer de l'auteur),
    /// `published`, `applied`, `closed`, `refused`.
    pub enum FeedbackStatus {
        Open = "open",
        Closed = "closed",
        Pending = "pending",
        Published = "published",
        Applied = "applied",
        Refused = "refused",
    }
}

text_enum! {
    /// « Comment c'était ? »
    pub enum Rating {
        Top = "top",
        Bof = "bof",
        Flop = "flop",
    }
}

/// Un retour sur une recette.
#[derive(Serialize, ToSchema, Clone, Debug)]
pub struct Feedback {
    pub id: Uuid,
    /// La phase, le geste ou la ligne visés ; nul : la recette entière (ou un élément qui n'existe plus).
    pub element_key: Option<Uuid>,
    pub kind: FeedbackKind,
    pub visibility: Visibility,
    pub status: FeedbackStatus,
    pub body: String,
    /// Par champ, l'ancienne et la nouvelle valeur : `{"timer_s": {"from": 720, "to": 840}}`.
    #[schema(value_type = Option<Object>)]
    pub changes: Option<Value>,
    pub author_name: String,
    /// Version de la recette sur laquelle il a été écrit.
    pub base_version: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(FromRow)]
struct FeedbackRow {
    id: Uuid,
    element_key: Option<Uuid>,
    #[sqlx(try_from = "String")]
    kind: FeedbackKind,
    #[sqlx(try_from = "String")]
    visibility: Visibility,
    #[sqlx(try_from = "String")]
    status: FeedbackStatus,
    body: String,
    changes: Option<Value>,
    author_name: String,
    base_version: i32,
    created_at: OffsetDateTime,
}

impl From<FeedbackRow> for Feedback {
    fn from(row: FeedbackRow) -> Self {
        Self {
            id: row.id,
            element_key: row.element_key,
            kind: row.kind,
            visibility: row.visibility,
            status: row.status,
            body: row.body,
            changes: row.changes,
            author_name: row.author_name,
            base_version: row.base_version,
            created_at: row.created_at,
        }
    }
}

/// Une clé qui ne désigne plus rien (étape retirée) : le retour remonte sur la recette entière.
fn attach(recipe: &Recipe, mut feedback: Feedback) -> Feedback {
    if feedback
        .element_key
        .is_some_and(|key| recipe.element(key).is_none())
    {
        feedback.element_key = None;
    }
    feedback
}

/// Les notes ouvertes du cahier de ce foyer sur cette recette, de la plus ancienne à la plus récente.
pub(crate) async fn open_notes(
    conn: &mut PgConnection,
    household_id: Uuid,
    recipe_id: Uuid,
    recipe: &Recipe,
) -> ApiResult<Vec<Feedback>> {
    let rows: Vec<FeedbackRow> = sqlx::query_as(
        "SELECT id, element_key, kind, visibility, status, body, changes, author_name, base_version, created_at
         FROM recipe_feedback
         WHERE recipe_id = $1 AND household_id = $2 AND visibility = 'household' AND status = 'open'
           AND parent_id IS NULL
         ORDER BY created_at, id",
    )
    .bind(recipe_id)
    .bind(household_id)
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().map(|r| attach(recipe, r.into())).collect())
}

/// Un texte de retour : de 1 à 2 000 caractères.
fn body(text: &str) -> ApiResult<String> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > 2000 {
        return Err(ApiError::invalid(
            "Le texte fait entre 1 et 2 000 caractères.",
        ));
    }
    Ok(text.to_owned())
}

/// Les changements demandés sur un élément, avec la valeur actuelle : `{champ: {from, to}}`. Une phase ou un geste :
/// le texte, le texte lu par Alexa, le minuteur ; une ligne : la quantité.
pub(crate) fn changes(
    element: Option<Element<'_>>,
    wanted: &Map<String, Value>,
) -> ApiResult<Value> {
    let Some(element) = element else {
        return Err(ApiError::invalid(
            "Un changement vise une étape, un geste ou une ligne d'ingrédient.",
        ));
    };
    let mut out = Map::new();
    for (field, to) in wanted {
        let (from, ok) = match (&element, field.as_str()) {
            (Element::Phase(p), "text") => (json!(p.text), is_text(to)),
            (Element::Step(s), "text") => (json!(s.text), is_text(to)),
            (Element::Phase(p), "spoken_text") => {
                (json!(p.spoken_text), to.is_null() || is_text(to))
            }
            (Element::Step(s), "spoken_text") => {
                (json!(s.spoken_text), to.is_null() || is_text(to))
            }
            (Element::Phase(p), "timer_s") => (json!(p.timer_s), is_seconds(to)),
            (Element::Step(s), "timer_s") => (json!(s.timer_s), is_seconds(to)),
            (Element::Line(l), "qty") => (json!(l.qty), to.as_f64().is_some_and(|q| q > 0.0)),
            _ => {
                return Err(ApiError::invalid(format!(
                    "« {field} » ne se change pas ici : le texte, le texte lu ou le minuteur d'une étape, la quantité d'une ligne."
                )));
            }
        };
        if !ok {
            return Err(ApiError::invalid(format!("« {field} » : valeur invalide.")));
        }
        out.insert(field.clone(), json!({"from": from, "to": to}));
    }
    if out.is_empty() {
        return Err(ApiError::invalid("Aucun changement."));
    }
    Ok(Value::Object(out))
}

fn is_text(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|t| !t.trim().is_empty() && t.chars().count() <= 4000)
}

fn is_seconds(value: &Value) -> bool {
    value
        .as_i64()
        .is_some_and(|s| s > 0 && s <= i64::from(i32::MAX))
}

/// Retour à écrire.
pub(crate) struct NewFeedback<'a> {
    pub recipe_id: Uuid,
    pub household_id: Uuid,
    pub author: &'a Author,
    pub element_key: Option<Uuid>,
    pub kind: FeedbackKind,
    pub visibility: Visibility,
    pub status: FeedbackStatus,
    pub body: String,
    pub changes: Option<Value>,
    pub base_version: i32,
}

pub(crate) async fn insert(conn: &mut PgConnection, new: NewFeedback<'_>) -> ApiResult<Feedback> {
    let row: FeedbackRow = sqlx::query_as(
        "INSERT INTO recipe_feedback (id, recipe_id, household_id, author_id, author_name, element_key, visibility,
                                      kind, status, body, changes, base_version)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
         RETURNING id, element_key, kind, visibility, status, body, changes, author_name, base_version, created_at",
    )
    .bind(Uuid::now_v7())
    .bind(new.recipe_id)
    .bind(new.household_id)
    .bind(new.author.id)
    .bind(&new.author.name)
    .bind(new.element_key)
    .bind(new.visibility.as_str())
    .bind(new.kind.as_str())
    .bind(new.status.as_str())
    .bind(&new.body)
    .bind(&new.changes)
    .bind(new.base_version)
    .fetch_one(conn)
    .await?;
    Ok(row.into())
}

// Routes

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct FeedbackQuery {
    /// Avec les notes fermées.
    #[serde(default)]
    pub closed: bool,
}

/// Les notes du cahier du foyer sur cette recette (les ouvertes ; avec `closed`, aussi les fermées).
#[utoipa::path(
    get,
    path = "/api/v1/households/{id}/recipes/{slug}/feedback",
    tag = "retours",
    params(("id" = Uuid, Path), ("slug" = String, Path), FeedbackQuery),
    responses(
        (status = 200, body = Vec<Feedback>),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn list_feedback(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, slug)): Path<(Uuid, String)>,
    Query(query): Query<FeedbackQuery>,
) -> ApiResult<Json<Vec<Feedback>>> {
    caller.require(&state.db, id, Scope::Read).await?;
    let mut conn = state.db.acquire().await?;
    let (recipe_id, _) = recipes::find(&mut conn, id, &slug)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let recipe = recipes::load_by_id(&state.db, id, recipe_id).await?;
    let rows: Vec<FeedbackRow> = sqlx::query_as(
        "SELECT id, element_key, kind, visibility, status, body, changes, author_name, base_version, created_at
         FROM recipe_feedback
         WHERE recipe_id = $1 AND household_id = $2 AND visibility = 'household' AND parent_id IS NULL
           AND (status = 'open' OR $3)
         ORDER BY created_at, id",
    )
    .bind(recipe_id)
    .bind(id)
    .bind(query.closed)
    .fetch_all(&mut *conn)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| attach(&recipe, r.into()))
            .collect(),
    ))
}

/// Une note à écrire.
#[derive(Deserialize, ToSchema)]
pub struct FeedbackInput {
    /// La phase, le geste ou la ligne ; sans : la recette entière.
    pub element_key: Option<Uuid>,
    pub body: String,
    /// Par champ, la nouvelle valeur (« chez nous : 14 min » : `{"timer_s": 840}`).
    #[schema(value_type = Option<Object>)]
    pub changes: Option<Map<String, Value>>,
}

/// Écrit une note dans le cahier du foyer : visible du foyer seul, jamais relue par personne d'autre.
#[utoipa::path(
    post,
    path = "/api/v1/households/{id}/recipes/{slug}/feedback",
    tag = "retours",
    params(("id" = Uuid, Path), ("slug" = String, Path)),
    request_body = FeedbackInput,
    responses(
        (status = 201, body = Feedback),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn create_feedback(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, slug)): Path<(Uuid, String)>,
    Json(input): Json<FeedbackInput>,
) -> ApiResult<(StatusCode, Json<Feedback>)> {
    caller.require(&state.db, id, Scope::RecipesWrite).await?;
    let mut conn = state.db.acquire().await?;
    let (recipe_id, _) = recipes::find(&mut conn, id, &slug)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let recipe = recipes::load_by_id(&state.db, id, recipe_id).await?;
    let element = match input.element_key {
        Some(key) => Some(recipe.element(key).ok_or_else(|| {
            ApiError::invalid("Cette étape n'existe pas (ou plus) dans la recette.")
        })?),
        None => None,
    };
    let changes = match &input.changes {
        Some(wanted) => Some(changes(element, wanted)?),
        None => None,
    };
    let author = caller.author(&state.db).await?;
    let feedback = insert(
        &mut conn,
        NewFeedback {
            recipe_id,
            household_id: id,
            author: &author,
            element_key: input.element_key,
            kind: FeedbackKind::Comment,
            visibility: Visibility::Household,
            status: FeedbackStatus::Open,
            body: body(&input.body)?,
            changes,
            base_version: recipe.version,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(feedback)))
}

/// Ce qu'on change sur une note.
#[derive(Deserialize, ToSchema)]
pub struct FeedbackUpdate {
    pub body: Option<String>,
    /// Fermer (pris en compte, ou plus d'actualité) ou rouvrir.
    pub closed: Option<bool>,
}

/// Une note du cahier de ce foyer : `(id, recette)`. 404 pour un retour d'un autre foyer.
async fn own_note(
    conn: &mut PgConnection,
    household_id: Uuid,
    feedback_id: Uuid,
) -> ApiResult<Uuid> {
    sqlx::query_scalar(
        "SELECT recipe_id FROM recipe_feedback
         WHERE id = $1 AND household_id = $2 AND visibility = 'household'",
    )
    .bind(feedback_id)
    .bind(household_id)
    .fetch_optional(conn)
    .await?
    .ok_or_else(ApiError::not_found)
}

/// Modifie, ferme ou rouvre une note du cahier (tout compte du foyer).
#[utoipa::path(
    patch,
    path = "/api/v1/households/{id}/feedback/{feedback_id}",
    tag = "retours",
    params(("id" = Uuid, Path), ("feedback_id" = Uuid, Path)),
    request_body = FeedbackUpdate,
    responses(
        (status = 200, body = Feedback),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn update_feedback(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, feedback_id)): Path<(Uuid, Uuid)>,
    Json(update): Json<FeedbackUpdate>,
) -> ApiResult<Json<Feedback>> {
    caller.require(&state.db, id, Scope::RecipesWrite).await?;
    let mut conn = state.db.acquire().await?;
    let recipe_id = own_note(&mut conn, id, feedback_id).await?;
    let text = update.body.as_deref().map(body).transpose()?;
    let status = update.closed.map(|closed| {
        if closed {
            FeedbackStatus::Closed
        } else {
            FeedbackStatus::Open
        }
    });
    let row: FeedbackRow = sqlx::query_as(
        "UPDATE recipe_feedback SET body = coalesce($2, body), status = coalesce($3, status), updated_at = now()
         WHERE id = $1
         RETURNING id, element_key, kind, visibility, status, body, changes, author_name, base_version, created_at",
    )
    .bind(feedback_id)
    .bind(text)
    .bind(status.map(FeedbackStatus::as_str))
    .fetch_one(&mut *conn)
    .await?;
    let recipe = recipes::load_by_id(&state.db, id, recipe_id).await?;
    Ok(Json(attach(&recipe, row.into())))
}

/// Supprime une note du cahier (tout compte du foyer).
#[utoipa::path(
    delete,
    path = "/api/v1/households/{id}/feedback/{feedback_id}",
    tag = "retours",
    params(("id" = Uuid, Path), ("feedback_id" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn delete_feedback(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, feedback_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    caller.require(&state.db, id, Scope::RecipesWrite).await?;
    let mut conn = state.db.acquire().await?;
    own_note(&mut conn, id, feedback_id).await?;
    sqlx::query("DELETE FROM recipe_feedback WHERE id = $1")
        .bind(feedback_id)
        .execute(&mut *conn)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Combien de fois la recette a été notée, par note (le foyer pour une recette du foyer, toute l'instance pour une
/// recette de la communauté).
#[derive(Serialize, ToSchema, FromRow, Default, Debug)]
pub struct RatingCounts {
    pub top: i64,
    pub bof: i64,
    pub flop: i64,
}

pub(crate) async fn counts(conn: &mut PgConnection, recipe_id: Uuid) -> ApiResult<RatingCounts> {
    Ok(sqlx::query_as(
        "SELECT count(*) FILTER (WHERE rating = 'top') AS top, count(*) FILTER (WHERE rating = 'bof') AS bof,
                count(*) FILTER (WHERE rating = 'flop') AS flop
         FROM meal_ratings WHERE recipe_id = $1",
    )
    .bind(recipe_id)
    .fetch_one(conn)
    .await?)
}

/// « Comment c'était ? » après un repas.
#[derive(Deserialize, ToSchema)]
pub struct RatingInput {
    /// Slug de la recette.
    pub recipe: String,
    #[schema(value_type = String, format = Date)]
    pub date: Day,
    pub meal: Meal,
    pub rating: Rating,
    /// Un mot, gardé comme note du cahier sur la recette.
    pub word: Option<String>,
}

/// Note un repas (une note par foyer, recette et repas : la dernière remplace la précédente).
#[utoipa::path(
    put,
    path = "/api/v1/households/{id}/ratings",
    tag = "retours",
    params(("id" = Uuid, Path)),
    request_body = RatingInput,
    responses(
        (status = 204),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn rate_meal(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Json(input): Json<RatingInput>,
) -> ApiResult<StatusCode> {
    caller.require(&state.db, id, Scope::RecipesWrite).await?;
    let author = caller.author(&state.db).await?;
    let mut tx = state.db.begin().await?;
    let (recipe_id, _) = recipes::find(&mut tx, id, &input.recipe)
        .await?
        .ok_or_else(|| ApiError::invalid(format!("Recette inconnue : {}.", input.recipe)))?;
    let word = input
        .word
        .as_deref()
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .map(body)
        .transpose()?;
    sqlx::query(
        "INSERT INTO meal_ratings (household_id, recipe_id, date, meal, rating, user_id)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (household_id, recipe_id, date, meal) DO UPDATE SET
             rating = EXCLUDED.rating, user_id = EXCLUDED.user_id, updated_at = now()",
    )
    .bind(id)
    .bind(recipe_id)
    .bind(input.date.0)
    .bind(input.meal.as_str())
    .bind(input.rating.as_str())
    .bind(author.id)
    .execute(&mut *tx)
    .await?;
    if let Some(word) = word {
        let version: i32 = sqlx::query_scalar("SELECT version FROM recipes WHERE id = $1")
            .bind(recipe_id)
            .fetch_one(&mut *tx)
            .await?;
        insert(
            &mut tx,
            NewFeedback {
                recipe_id,
                household_id: id,
                author: &author,
                element_key: None,
                kind: FeedbackKind::Comment,
                visibility: Visibility::Household,
                status: FeedbackStatus::Open,
                body: word,
                changes: None,
                base_version: version,
            },
        )
        .await?;
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
