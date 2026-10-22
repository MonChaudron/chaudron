//! Retours sur une recette. Le cahier du foyer : des notes privées, sur la recette entière ou sur une étape (par sa
//! clé), sur n'importe quelle recette que le foyer voit ; elles ne passent jamais en revue. Une note peut porter un
//! changement (« chez nous : 14 min »), affiché à côté du texte de l'étape sans toucher la recette.
//! Sur une recette de la communauté, des retours pour tous : commentaires, propositions de changement par étape,
//! variantes tentées, fils et « +1 ». Un retour n'est visible que du foyer de son auteur tant que l'admin ne l'a pas
//! publié ; l'admin le publie, l'applique (une nouvelle version), le ferme ou le refuse, et l'auteur est prévenu.
//! Et « comment c'était ? » : une note par foyer et par repas.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, patch, post, put},
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
    notifications,
    recipes::{self, Element, Recipe, RecipeInput},
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
        .route(
            "/households/{id}/feedback/{feedback_id}/replies",
            post(reply),
        )
        .route(
            "/households/{id}/feedback/{feedback_id}/support",
            post(support).delete(unsupport),
        )
        .route("/community/feedback/{feedback_id}/publish", post(publish))
        .route("/community/feedback/{feedback_id}/apply", post(apply))
        .route("/community/feedback/{feedback_id}/close", post(close))
        .route("/community/feedback/{feedback_id}/refuse", post(refuse))
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
    /// Le retour auquel celui-ci répond (fil de discussion).
    pub parent_id: Option<Uuid>,
    /// La phase, le geste ou la ligne visés ; nul : la recette entière (ou un élément qui n'existe plus).
    pub element_key: Option<Uuid>,
    pub kind: FeedbackKind,
    pub visibility: Visibility,
    pub status: FeedbackStatus,
    pub body: String,
    /// Par champ, l'ancienne et la nouvelle valeur : `{"timer_s": {"from": 720, "to": 840}}`.
    #[schema(value_type = Option<Object>)]
    pub changes: Option<Value>,
    /// Variante tentée : `{"replaced": code, "by": code, "verdict": "good" | "bad"}`.
    #[schema(value_type = Option<Object>)]
    pub tried: Option<Value>,
    pub author_name: String,
    /// Le foyer de l'auteur (nul : foyer supprimé).
    pub household_name: Option<String>,
    /// Écrit par le foyer qui lit.
    pub mine: bool,
    /// « +1 » des foyers.
    pub supports: i64,
    /// Le foyer qui lit a appuyé.
    pub supported: bool,
    /// Proposition écrite sur une étape qui a changé depuis : à revoir avant de l'appliquer.
    pub stale: bool,
    /// Le mot de l'admin (refus, fermeture).
    pub review_message: Option<String>,
    /// Version de la recette sur laquelle il a été écrit.
    pub base_version: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(FromRow)]
struct FeedbackRow {
    id: Uuid,
    parent_id: Option<Uuid>,
    element_key: Option<Uuid>,
    #[sqlx(try_from = "String")]
    kind: FeedbackKind,
    #[sqlx(try_from = "String")]
    visibility: Visibility,
    #[sqlx(try_from = "String")]
    status: FeedbackStatus,
    body: String,
    changes: Option<Value>,
    tried: Option<Value>,
    author_name: String,
    household_name: Option<String>,
    mine: bool,
    supports: i64,
    supported: bool,
    review_message: Option<String>,
    base_version: i32,
    created_at: OffsetDateTime,
}

impl From<FeedbackRow> for Feedback {
    fn from(row: FeedbackRow) -> Self {
        Self {
            id: row.id,
            parent_id: row.parent_id,
            element_key: row.element_key,
            kind: row.kind,
            visibility: row.visibility,
            status: row.status,
            body: row.body,
            changes: row.changes,
            tried: row.tried,
            author_name: row.author_name,
            household_name: row.household_name,
            mine: row.mine,
            supports: row.supports,
            supported: row.supported,
            stale: false,
            review_message: row.review_message,
            base_version: row.base_version,
            created_at: row.created_at,
        }
    }
}

/// Les retours, vus par un foyer (`$2`) : `mine`, `supported`.
const SELECT: &str = "SELECT f.id, f.parent_id, f.element_key, f.kind, f.visibility, f.status, f.body, f.changes,
        f.tried, f.author_name, h.name AS household_name, coalesce(f.household_id = $2, false) AS mine,
        (SELECT count(*) FROM feedback_support s WHERE s.feedback_id = f.id) AS supports,
        EXISTS (SELECT 1 FROM feedback_support s WHERE s.feedback_id = f.id AND s.household_id = $2) AS supported,
        f.review_message, f.base_version, f.created_at
    FROM recipe_feedback f LEFT JOIN households h ON h.id = f.household_id";

/// Ce qu'un foyer (`$2`) voit : son cahier ; dans la communauté, ce que l'admin a publié (appliqué, fermé), et ses
/// propres retours en attente ou refusés. Le seul filtre de visibilité : fiche, recette lue, cuisson, Alexa.
const VISIBLE: &str = "((f.visibility = 'household' AND f.household_id = $2)
    OR (f.visibility = 'community' AND (f.status IN ('published', 'applied', 'closed') OR f.household_id = $2)))";

/// La valeur actuelle d'un champ d'un élément (ce qu'une proposition remplace).
fn current(element: &Element<'_>, field: &str) -> Option<Value> {
    Some(match (element, field) {
        (Element::Phase(p), "text") => json!(p.text),
        (Element::Step(s), "text") => json!(s.text),
        (Element::Phase(p), "spoken_text") => json!(p.spoken_text),
        (Element::Step(s), "spoken_text") => json!(s.spoken_text),
        (Element::Phase(p), "timer_s") => json!(p.timer_s),
        (Element::Step(s), "timer_s") => json!(s.timer_s),
        (Element::Line(l), "qty") => json!(l.qty),
        _ => return None,
    })
}

/// Deux valeurs JSON égales (les nombres comparés comme tels : 720 et 720.0).
fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x - y).abs() < 1e-6,
        _ => a == b,
    }
}

/// Une proposition encore ouverte dont l'étape a changé (ou disparu) depuis qu'elle a été écrite.
fn is_stale(recipe: &Recipe, feedback: &Feedback) -> bool {
    if feedback.kind != FeedbackKind::Proposal
        || !matches!(
            feedback.status,
            FeedbackStatus::Pending | FeedbackStatus::Published
        )
    {
        return false;
    }
    let Some(element) = feedback.element_key.and_then(|k| recipe.element(k)) else {
        return true;
    };
    let Some(Value::Object(changes)) = &feedback.changes else {
        return false;
    };
    changes.iter().any(|(field, change)| {
        current(&element, field).is_none_or(|now| !same(&now, &change["from"]))
    })
}

/// Rattache un retour à la version courante : une clé qui ne désigne plus rien remonte sur la recette entière (une
/// proposition garde la sienne, marquée `stale`).
pub(crate) fn finish(recipe: &Recipe, mut feedback: Feedback) -> Feedback {
    feedback.stale = is_stale(recipe, &feedback);
    if feedback.kind != FeedbackKind::Proposal
        && feedback
            .element_key
            .is_some_and(|key| recipe.element(key).is_none())
    {
        feedback.element_key = None;
    }
    feedback
}

/// Les notes que la recette lue porte pour ce foyer (fiche, agenda, cuisson, Alexa) : son cahier ouvert, et les
/// commentaires de la communauté publiés (ou les siens en attente). Pas les propositions, qui restent sur la fiche.
pub(crate) async fn open_notes(
    conn: &mut PgConnection,
    household_id: Uuid,
    recipe_id: Uuid,
    recipe: &Recipe,
) -> ApiResult<Vec<Feedback>> {
    let rows: Vec<FeedbackRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE f.recipe_id = $1 AND {VISIBLE} AND f.parent_id IS NULL AND f.kind <> 'proposal'
           AND f.status IN ('open', 'published', 'pending')
         ORDER BY f.created_at, f.id"
    )))
    .bind(recipe_id)
    .bind(household_id)
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().map(|r| finish(recipe, r.into())).collect())
}

/// Un retour vu par un foyer (sans le rattacher à la recette).
pub(crate) async fn fetch(conn: &mut PgConnection, id: Uuid, viewer: Uuid) -> ApiResult<Feedback> {
    let row: FeedbackRow = sqlx::query_as(sqlx::AssertSqlSafe(format!("{SELECT} WHERE f.id = $1")))
        .bind(id)
        .bind(viewer)
        .fetch_optional(conn)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(row.into())
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
        let Some(from) = current(&element, field) else {
            return Err(ApiError::invalid(format!(
                "« {field} » ne se change pas ici : le texte, le texte lu ou le minuteur d'une étape, la quantité d'une ligne."
            )));
        };
        let ok = match field.as_str() {
            "text" => is_text(to),
            "spoken_text" => to.is_null() || is_text(to),
            "timer_s" => is_seconds(to),
            _ => to.as_f64().is_some_and(|q| q > 0.0),
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

/// Écrit un retour ; le renvoie vu par son foyer.
pub(crate) async fn insert(conn: &mut PgConnection, new: NewFeedback<'_>) -> ApiResult<Feedback> {
    insert_full(conn, new, None, None).await
}

async fn insert_full(
    conn: &mut PgConnection,
    new: NewFeedback<'_>,
    parent_id: Option<Uuid>,
    tried: Option<Value>,
) -> ApiResult<Feedback> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO recipe_feedback (id, recipe_id, household_id, author_id, author_name, element_key, visibility,
                                      kind, status, body, changes, base_version, parent_id, tried)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(id)
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
    .bind(parent_id)
    .bind(tried)
    .execute(&mut *conn)
    .await?;
    fetch(conn, id, new.household_id).await
}

// Routes

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct FeedbackQuery {
    /// Avec les discussions fermées (et les propositions appliquées).
    #[serde(default)]
    pub closed: bool,
}

/// Les retours que le foyer voit sur cette recette, fils compris (`parent_id`) : son cahier et, sur une recette de la
/// communauté, les retours publiés et les siens (en attente, refusés). Les fermés avec `closed`.
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
    let rows: Vec<FeedbackRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE f.recipe_id = $1 AND {VISIBLE}
           AND (f.status IN ('open', 'pending', 'published') OR (f.status = 'refused' AND f.household_id = $2) OR $3)
         ORDER BY f.created_at, f.id"
    )))
    .bind(recipe_id)
    .bind(id)
    .bind(query.closed)
    .fetch_all(&mut *conn)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| finish(&recipe, r.into()))
            .collect(),
    ))
}

/// Une variante tentée : l'ingrédient remplacé, le remplaçant (codes du référentiel), et si c'était bien.
#[derive(Deserialize, ToSchema)]
pub struct TriedInput {
    pub replaced: String,
    pub by: String,
    /// `good` ou `bad`.
    pub verdict: String,
}

/// Un retour à écrire.
#[derive(Deserialize, ToSchema)]
pub struct FeedbackInput {
    /// La phase, le geste ou la ligne ; sans : la recette entière.
    pub element_key: Option<Uuid>,
    pub body: String,
    /// Par champ, la nouvelle valeur (« chez nous : 14 min » : `{"timer_s": 840}`).
    #[schema(value_type = Option<Object>)]
    pub changes: Option<Map<String, Value>>,
    /// `household` (le cahier du foyer, par défaut) ou `community` (une recette de la communauté : relu par
    /// l'admin avant d'être publié).
    pub visibility: Option<Visibility>,
    /// `comment` par défaut ; `proposal` (communauté, une étape et ses changements) ; `tried`.
    pub kind: Option<FeedbackKind>,
    pub tried: Option<TriedInput>,
}

/// Écrit un retour : une note du cahier du foyer (visible du foyer seul), ou, sur une recette de la communauté, un
/// commentaire, une proposition de changement ou une variante tentée, en attente de l'admin.
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
    let (recipe_id, owner) = recipes::find(&mut conn, id, &slug)
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
    let visibility = input.visibility.unwrap_or(Visibility::Household);
    let kind = input.kind.unwrap_or(FeedbackKind::Comment);
    let community = visibility == Visibility::Community;
    if community && owner.is_some() {
        return Err(ApiError::invalid(
            "Une recette du foyer se modifie directement : écris une note dans le cahier.",
        ));
    }
    if !community && kind != FeedbackKind::Comment {
        return Err(ApiError::invalid(
            "Le cahier du foyer prend des notes ; propositions et variantes tentées vont à la communauté.",
        ));
    }
    if kind == FeedbackKind::Proposal && changes.is_none() {
        return Err(ApiError::invalid(
            "Une proposition dit ce qui change sur l'étape.",
        ));
    }
    let tried = match (&input.tried, kind) {
        (Some(t), FeedbackKind::Tried) => {
            let known: i64 =
                sqlx::query_scalar("SELECT count(*) FROM ingredients WHERE code = ANY($1)")
                    .bind([t.replaced.as_str(), t.by.as_str()])
                    .fetch_one(&mut *conn)
                    .await?;
            if known != 2 || t.replaced == t.by || !["good", "bad"].contains(&t.verdict.as_str()) {
                return Err(ApiError::invalid(
                    "Variante tentée : deux ingrédients du référentiel, et `good` ou `bad`.",
                ));
            }
            Some(json!({"replaced": t.replaced, "by": t.by, "verdict": t.verdict}))
        }
        (None, FeedbackKind::Tried) => {
            return Err(ApiError::invalid(
                "Variante tentée : quel ingrédient, remplacé par quoi.",
            ));
        }
        _ => None,
    };
    let author = caller.author(&state.db).await?;
    let feedback = insert_full(
        &mut conn,
        NewFeedback {
            recipe_id,
            household_id: id,
            author: &author,
            element_key: input.element_key,
            kind,
            visibility,
            status: if community {
                FeedbackStatus::Pending
            } else {
                FeedbackStatus::Open
            },
            body: body(&input.body)?,
            changes,
            base_version: recipe.version,
        },
        None,
        tried,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(finish(&recipe, feedback))))
}

/// Ce qu'on change sur un retour du foyer.
#[derive(Deserialize, ToSchema)]
pub struct FeedbackUpdate {
    pub body: Option<String>,
    /// Fermer (pris en compte, ou plus d'actualité) ou rouvrir une note du cahier.
    pub closed: Option<bool>,
    /// `community` : envoyer une note du cahier à la communauté (en attente de l'admin) ; `household` : garder un
    /// retour refusé comme note du cahier.
    pub visibility: Option<Visibility>,
}

/// Un retour de ce foyer : (recette, visibilité, statut, porte des changements sur une étape). 404 pour un retour
/// d'un autre foyer.
async fn own(
    conn: &mut PgConnection,
    household_id: Uuid,
    feedback_id: Uuid,
) -> ApiResult<(Uuid, Visibility, FeedbackStatus, bool)> {
    let row: (Uuid, String, String, bool) = sqlx::query_as(
        "SELECT recipe_id, visibility, status, changes IS NOT NULL AND element_key IS NOT NULL
         FROM recipe_feedback WHERE id = $1 AND household_id = $2",
    )
    .bind(feedback_id)
    .bind(household_id)
    .fetch_optional(conn)
    .await?
    .ok_or_else(ApiError::not_found)?;
    Ok((
        row.0,
        Visibility::try_from(row.1).map_err(|_| ApiError::internal())?,
        FeedbackStatus::try_from(row.2).map_err(|_| ApiError::internal())?,
        row.3,
    ))
}

/// Modifie, ferme ou rouvre une note du cahier (tout compte du foyer) ; l'envoie à la communauté ; garde un retour
/// refusé comme note.
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
    let (recipe_id, visibility, status, with_changes) = own(&mut conn, id, feedback_id).await?;
    match (visibility, update.visibility) {
        (Visibility::Household, Some(Visibility::Community)) => {
            let community: bool =
                sqlx::query_scalar("SELECT household_id IS NULL FROM recipes WHERE id = $1")
                    .bind(recipe_id)
                    .fetch_one(&mut *conn)
                    .await?;
            if !community {
                return Err(ApiError::invalid(
                    "Une recette du foyer n'a pas de communauté : la note reste au cahier.",
                ));
            }
            let kind = if with_changes {
                FeedbackKind::Proposal
            } else {
                FeedbackKind::Comment
            };
            sqlx::query(
                "UPDATE recipe_feedback SET visibility = 'community', status = 'pending', kind = $2,
                        updated_at = now()
                 WHERE id = $1",
            )
            .bind(feedback_id)
            .bind(kind.as_str())
            .execute(&mut *conn)
            .await?;
        }
        (Visibility::Community, Some(Visibility::Household)) => {
            if status != FeedbackStatus::Refused {
                return Err(ApiError::invalid(
                    "Seul un retour refusé se garde comme note du cahier.",
                ));
            }
            sqlx::query(
                "UPDATE recipe_feedback SET visibility = 'household', status = 'open', kind = 'comment',
                        updated_at = now()
                 WHERE id = $1",
            )
            .bind(feedback_id)
            .execute(&mut *conn)
            .await?;
        }
        (Visibility::Community, _) if update.body.is_some() || update.closed.is_some() => {
            return Err(ApiError::invalid(
                "Un retour à la communauté ne se modifie plus : écris-en un autre, ou réponds.",
            ));
        }
        _ => {}
    }
    if visibility == Visibility::Household && update.visibility.is_none() {
        let text = update.body.as_deref().map(body).transpose()?;
        let status = update.closed.map(|closed| {
            if closed {
                FeedbackStatus::Closed
            } else {
                FeedbackStatus::Open
            }
        });
        sqlx::query(
            "UPDATE recipe_feedback SET body = coalesce($2, body), status = coalesce($3, status), updated_at = now()
             WHERE id = $1",
        )
        .bind(feedback_id)
        .bind(text)
        .bind(status.map(FeedbackStatus::as_str))
        .execute(&mut *conn)
        .await?;
    }
    let feedback = fetch(&mut conn, feedback_id, id).await?;
    let recipe = recipes::load_by_id(&state.db, id, recipe_id).await?;
    Ok(Json(finish(&recipe, feedback)))
}

/// Supprime une note du cahier, ou retire un retour encore en attente (tout compte du foyer).
#[utoipa::path(
    delete,
    path = "/api/v1/households/{id}/feedback/{feedback_id}",
    tag = "retours",
    params(("id" = Uuid, Path), ("feedback_id" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn delete_feedback(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, feedback_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    caller.require(&state.db, id, Scope::RecipesWrite).await?;
    let mut conn = state.db.acquire().await?;
    let (_, visibility, status, _) = own(&mut conn, id, feedback_id).await?;
    if visibility == Visibility::Community && status != FeedbackStatus::Pending {
        return Err(ApiError::invalid(
            "Un retour publié ou revu par l'admin reste : il fait partie de l'histoire de la recette.",
        ));
    }
    sqlx::query("DELETE FROM recipe_feedback WHERE id = $1")
        .bind(feedback_id)
        .execute(&mut *conn)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Une réponse dans un fil.
#[derive(Deserialize, ToSchema)]
pub struct ReplyInput {
    pub body: String,
}

/// Un retour que ce foyer voit : (recette, foyer de l'auteur, visibilité, statut).
async fn visible(
    conn: &mut PgConnection,
    household_id: Uuid,
    feedback_id: Uuid,
) -> ApiResult<(Uuid, Option<Uuid>, Visibility, FeedbackStatus)> {
    let row: (Uuid, Option<Uuid>, String, String) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT f.recipe_id, f.household_id, f.visibility, f.status FROM recipe_feedback f
         WHERE f.id = $1 AND {VISIBLE}"
    )))
    .bind(feedback_id)
    .bind(household_id)
    .fetch_optional(conn)
    .await?
    .ok_or_else(ApiError::not_found)?;
    Ok((
        row.0,
        row.1,
        Visibility::try_from(row.2).map_err(|_| ApiError::internal())?,
        FeedbackStatus::try_from(row.3).map_err(|_| ApiError::internal())?,
    ))
}

/// Répond dans un fil. Dans la communauté, la réponse d'un autre foyer attend l'admin, comme un premier retour ; celle
/// du foyer de l'auteur, ou de l'admin, est publiée d'emblée.
#[utoipa::path(
    post,
    path = "/api/v1/households/{id}/feedback/{feedback_id}/replies",
    tag = "retours",
    params(("id" = Uuid, Path), ("feedback_id" = Uuid, Path)),
    request_body = ReplyInput,
    responses(
        (status = 201, body = Feedback),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn reply(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, feedback_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<ReplyInput>,
) -> ApiResult<(StatusCode, Json<Feedback>)> {
    caller.require(&state.db, id, Scope::RecipesWrite).await?;
    let mut conn = state.db.acquire().await?;
    let (recipe_id, parent_household, visibility, status) =
        visible(&mut conn, id, feedback_id).await?;
    if !matches!(
        status,
        FeedbackStatus::Open | FeedbackStatus::Pending | FeedbackStatus::Published
    ) {
        return Err(ApiError::invalid("Cette discussion est fermée."));
    }
    let admin = match &caller {
        Caller::User(user) => crate::auth::is_admin(&state, *user).await?,
        Caller::Token(_) => false,
    };
    // Publiée d'emblée : la réponse de l'admin, ou celle du foyer de l'auteur dans un fil déjà publié. Sous un retour
    // encore en attente, elle attend aussi : rien ne devient visible de tous sans être relu.
    let status = match visibility {
        Visibility::Household => FeedbackStatus::Open,
        Visibility::Community
            if admin || (parent_household == Some(id) && status == FeedbackStatus::Published) =>
        {
            FeedbackStatus::Published
        }
        Visibility::Community => FeedbackStatus::Pending,
    };
    let version: i32 = sqlx::query_scalar("SELECT version FROM recipes WHERE id = $1")
        .bind(recipe_id)
        .fetch_one(&mut *conn)
        .await?;
    let author = caller.author(&state.db).await?;
    let feedback = insert_full(
        &mut conn,
        NewFeedback {
            recipe_id,
            household_id: id,
            author: &author,
            element_key: None,
            kind: FeedbackKind::Comment,
            visibility,
            status,
            body: body(&input.body)?,
            changes: None,
            base_version: version,
        },
        Some(feedback_id),
        None,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(feedback)))
}

/// Appuie un retour publié de la communauté (« +1 », « pareil chez nous ») : un par foyer.
#[utoipa::path(
    post,
    path = "/api/v1/households/{id}/feedback/{feedback_id}/support",
    tag = "retours",
    params(("id" = Uuid, Path), ("feedback_id" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn support(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, feedback_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    caller.require(&state.db, id, Scope::RecipesWrite).await?;
    let mut conn = state.db.acquire().await?;
    let (_, _, visibility, status) = visible(&mut conn, id, feedback_id).await?;
    if visibility != Visibility::Community || status != FeedbackStatus::Published {
        return Err(ApiError::invalid(
            "On appuie un retour publié de la communauté.",
        ));
    }
    sqlx::query(
        "INSERT INTO feedback_support (feedback_id, household_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(feedback_id)
    .bind(id)
    .execute(&mut *conn)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Retire son appui.
#[utoipa::path(
    delete,
    path = "/api/v1/households/{id}/feedback/{feedback_id}/support",
    tag = "retours",
    params(("id" = Uuid, Path), ("feedback_id" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn unsupport(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, feedback_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    caller.require(&state.db, id, Scope::RecipesWrite).await?;
    sqlx::query("DELETE FROM feedback_support WHERE feedback_id = $1 AND household_id = $2")
        .bind(feedback_id)
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// Revue (admin de l'instance)

/// Une recette de la communauté et ses retours qui attendent l'admin.
#[derive(Serialize, ToSchema)]
pub struct ReviewRecipe {
    pub slug: String,
    pub title: String,
    /// Retours en attente (premiers retours et réponses), et propositions publiées pas encore appliquées.
    pub feedback: Vec<Feedback>,
}

/// Les retours de la communauté à revoir, par recette.
pub(crate) async fn review_list(conn: &mut PgConnection) -> ApiResult<Vec<ReviewRecipe>> {
    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT f.recipe_id, f.id FROM recipe_feedback f
         WHERE f.visibility = 'community'
           AND (f.status = 'pending' OR (f.kind = 'proposal' AND f.status = 'published'))
         ORDER BY f.created_at, f.id",
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut out: Vec<(Uuid, ReviewRecipe, Recipe)> = Vec::new();
    for (recipe_id, feedback_id) in rows {
        if !out.iter().any(|(id, ..)| *id == recipe_id) {
            let recipe = recipes::load_plain(conn, recipe_id).await?;
            out.push((
                recipe_id,
                ReviewRecipe {
                    slug: recipe.slug.clone(),
                    title: recipe.title.clone(),
                    feedback: Vec::new(),
                },
                recipe,
            ));
        }
        let feedback = fetch(conn, feedback_id, Uuid::nil()).await?;
        if let Some((_, review, recipe)) = out.iter_mut().find(|(id, ..)| *id == recipe_id) {
            review.feedback.push(finish(recipe, feedback));
        }
    }
    Ok(out.into_iter().map(|(_, review, _)| review).collect())
}

/// Ce que l'admin répond.
#[derive(Deserialize, ToSchema)]
pub struct ReviewInput {
    /// Un mot pour l'auteur (obligatoire pour refuser).
    pub message: Option<String>,
    /// « Modifier puis appliquer » : la recette telle que l'admin la publie.
    pub recipe: Option<RecipeInput>,
}

/// Le retour à revoir, verrouillé.
struct Reviewed {
    recipe_id: Uuid,
    kind: FeedbackKind,
    status: FeedbackStatus,
    element_key: Option<Uuid>,
    changes: Option<Value>,
    author_id: Option<Uuid>,
    household: Option<String>,
}

type ReviewedRow = (
    Uuid,
    String,
    String,
    Option<Uuid>,
    Option<Value>,
    Option<Uuid>,
    Option<String>,
);

async fn reviewed(conn: &mut PgConnection, feedback_id: Uuid) -> ApiResult<Reviewed> {
    let row: ReviewedRow = sqlx::query_as(
        "SELECT f.recipe_id, f.kind, f.status, f.element_key, f.changes, f.author_id, h.name
         FROM recipe_feedback f LEFT JOIN households h ON h.id = f.household_id
         WHERE f.id = $1 AND f.visibility = 'community' FOR UPDATE OF f",
    )
    .bind(feedback_id)
    .fetch_optional(conn)
    .await?
    .ok_or_else(ApiError::not_found)?;
    Ok(Reviewed {
        recipe_id: row.0,
        kind: FeedbackKind::try_from(row.1).map_err(|_| ApiError::internal())?,
        status: FeedbackStatus::try_from(row.2).map_err(|_| ApiError::internal())?,
        element_key: row.3,
        changes: row.4,
        author_id: row.5,
        household: row.6,
    })
}

fn word(message: Option<&str>) -> ApiResult<Option<String>> {
    message
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(body)
        .transpose()
}

fn already() -> ApiError {
    ApiError::conflict("already_reviewed", "Ce retour a déjà une réponse.")
}

async fn conclude(
    conn: &mut PgConnection,
    feedback_id: Uuid,
    status: FeedbackStatus,
    message: Option<&str>,
    admin: Uuid,
) -> ApiResult<()> {
    sqlx::query(
        "UPDATE recipe_feedback SET status = $2, review_message = $3, reviewed_by = $4, reviewed_at = now(),
                updated_at = now()
         WHERE id = $1",
    )
    .bind(feedback_id)
    .bind(status.as_str())
    .bind(message)
    .bind(admin)
    .execute(conn)
    .await?;
    Ok(())
}

async fn title(conn: &mut PgConnection, recipe_id: Uuid) -> ApiResult<String> {
    Ok(
        sqlx::query_scalar("SELECT title FROM recipes WHERE id = $1")
            .bind(recipe_id)
            .fetch_one(conn)
            .await?,
    )
}

/// Publie un retour en attente (admin) : visible par tous ceux qui voient la recette, ouvert aux réponses et aux
/// « +1 ».
#[utoipa::path(
    post,
    path = "/api/v1/community/feedback/{feedback_id}/publish",
    tag = "communauté",
    params(("feedback_id" = Uuid, Path)),
    request_body = ReviewInput,
    responses(
        (status = 204),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
    )
)]
pub async fn publish(
    State(state): State<AppState>,
    caller: Caller,
    Path(feedback_id): Path<Uuid>,
    Json(input): Json<ReviewInput>,
) -> ApiResult<StatusCode> {
    let admin = caller.require_admin(&state).await?;
    let mut tx = state.db.begin().await?;
    let r = reviewed(&mut tx, feedback_id).await?;
    if r.status != FeedbackStatus::Pending {
        return Err(already());
    }
    let message = word(input.message.as_deref())?;
    conclude(
        &mut tx,
        feedback_id,
        FeedbackStatus::Published,
        message.as_deref(),
        admin,
    )
    .await?;
    let text = format!(
        "Ton retour sur « {} » est publié.",
        title(&mut tx, r.recipe_id).await?
    );
    notifications::notify(
        &mut tx,
        r.author_id,
        "feedback_published",
        Some(r.recipe_id),
        &text,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Applique une proposition (admin) : une nouvelle version de la recette, et la discussion se ferme. Si l'étape a
/// changé depuis, 409 `step_changed` : l'admin la revoit et envoie la recette modifiée (« Modifier puis appliquer »).
#[utoipa::path(
    post,
    path = "/api/v1/community/feedback/{feedback_id}/apply",
    tag = "communauté",
    params(("feedback_id" = Uuid, Path)),
    request_body = ReviewInput,
    responses(
        (status = 200, body = Recipe),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn apply(
    State(state): State<AppState>,
    caller: Caller,
    Path(feedback_id): Path<Uuid>,
    Json(input): Json<ReviewInput>,
) -> ApiResult<Json<Recipe>> {
    let admin = caller.require_admin(&state).await?;
    let author = caller.author(&state.db).await?;
    let mut tx = state.db.begin().await?;
    let r = reviewed(&mut tx, feedback_id).await?;
    if !matches!(
        r.status,
        FeedbackStatus::Pending | FeedbackStatus::Published
    ) {
        return Err(already());
    }
    if r.kind != FeedbackKind::Proposal && input.recipe.is_none() {
        return Err(ApiError::invalid(
            "Seule une proposition s'applique telle quelle ; pour un commentaire, envoie la recette modifiée.",
        ));
    }
    let recipe = recipes::load_plain(&mut tx, r.recipe_id).await?;
    let mut content = match input.recipe {
        Some(edited) => edited,
        None => {
            let Some(Value::Object(changes)) = &r.changes else {
                return Err(ApiError::invalid("Cette proposition ne change rien."));
            };
            let element = r.element_key.and_then(|k| recipe.element(k));
            let changed = element.as_ref().is_none_or(|el| {
                changes
                    .iter()
                    .any(|(field, c)| current(el, field).is_none_or(|now| !same(&now, &c["from"])))
            });
            let (false, Some(key)) = (changed, r.element_key) else {
                return Err(ApiError::conflict(
                    "step_changed",
                    "L'étape a changé depuis la proposition : revois-la, puis envoie la recette modifiée.",
                ));
            };
            let mut value =
                serde_json::to_value(recipe.to_input()).map_err(|_| ApiError::internal())?;
            set_fields(&mut value, &key.to_string(), changes);
            serde_json::from_value(value).map_err(|_| ApiError::internal())?
        }
    };
    content.slug = None;
    content.message = Some(format!(
        "Proposition du foyer {} intégrée",
        r.household.as_deref().unwrap_or("supprimé")
    ));
    recipes::save(&mut tx, None, &recipe.slug, &content, &author).await?;
    let message = word(input.message.as_deref())?;
    conclude(
        &mut tx,
        feedback_id,
        FeedbackStatus::Applied,
        message.as_deref(),
        admin,
    )
    .await?;
    let text = format!("« {} » a été amélioré avec ton retour.", recipe.title);
    notifications::notify(
        &mut tx,
        r.author_id,
        "feedback_applied",
        Some(r.recipe_id),
        &text,
    )
    .await?;
    let saved = recipes::load_plain(&mut tx, r.recipe_id).await?;
    tx.commit().await?;
    Ok(Json(saved))
}

/// Dans la recette (en JSON), donne à l'élément de cette clé les valeurs proposées.
fn set_fields(value: &mut Value, key: &str, changes: &Map<String, Value>) {
    match value {
        Value::Object(object) => {
            if object.get("key").and_then(Value::as_str) == Some(key) {
                for (field, change) in changes {
                    object.insert(field.clone(), change["to"].clone());
                }
                return;
            }
            for child in object.values_mut() {
                set_fields(child, key, changes);
            }
        }
        Value::Array(items) => {
            for item in items {
                set_fields(item, key, changes);
            }
        }
        _ => {}
    }
}

/// Ferme un retour (admin) : pris en compte dans une modification, ou rien à faire. La discussion disparaît de
/// l'affichage (on la revoit avec les fermées).
#[utoipa::path(
    post,
    path = "/api/v1/community/feedback/{feedback_id}/close",
    tag = "communauté",
    params(("feedback_id" = Uuid, Path)),
    request_body = ReviewInput,
    responses(
        (status = 204),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
    )
)]
pub async fn close(
    State(state): State<AppState>,
    caller: Caller,
    Path(feedback_id): Path<Uuid>,
    Json(input): Json<ReviewInput>,
) -> ApiResult<StatusCode> {
    let admin = caller.require_admin(&state).await?;
    let mut tx = state.db.begin().await?;
    let r = reviewed(&mut tx, feedback_id).await?;
    if !matches!(
        r.status,
        FeedbackStatus::Pending | FeedbackStatus::Published
    ) {
        return Err(already());
    }
    let message = word(input.message.as_deref())?;
    conclude(
        &mut tx,
        feedback_id,
        FeedbackStatus::Closed,
        message.as_deref(),
        admin,
    )
    .await?;
    let recipe = title(&mut tx, r.recipe_id).await?;
    let text = match &message {
        Some(m) => format!("« {recipe} » : ton retour a été pris en compte. {m}"),
        None => format!("« {recipe} » : ton retour a été pris en compte."),
    };
    notifications::notify(
        &mut tx,
        r.author_id,
        "feedback_closed",
        Some(r.recipe_id),
        &text,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Refuse un retour (admin), avec un mot ; son foyer peut le garder comme note du cahier.
#[utoipa::path(
    post,
    path = "/api/v1/community/feedback/{feedback_id}/refuse",
    tag = "communauté",
    params(("feedback_id" = Uuid, Path)),
    request_body = ReviewInput,
    responses(
        (status = 204),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn refuse(
    State(state): State<AppState>,
    caller: Caller,
    Path(feedback_id): Path<Uuid>,
    Json(input): Json<ReviewInput>,
) -> ApiResult<StatusCode> {
    let admin = caller.require_admin(&state).await?;
    let Some(message) = word(input.message.as_deref())? else {
        return Err(ApiError::invalid(
            "Un mot pour l'auteur : pourquoi pas cette fois.",
        ));
    };
    let mut tx = state.db.begin().await?;
    let r = reviewed(&mut tx, feedback_id).await?;
    if !matches!(
        r.status,
        FeedbackStatus::Pending | FeedbackStatus::Published
    ) {
        return Err(already());
    }
    conclude(
        &mut tx,
        feedback_id,
        FeedbackStatus::Refused,
        Some(&message),
        admin,
    )
    .await?;
    let text = format!(
        "« {} » : ton retour n'a pas été retenu. {message}",
        title(&mut tx, r.recipe_id).await?
    );
    notifications::notify(
        &mut tx,
        r.author_id,
        "feedback_refused",
        Some(r.recipe_id),
        &text,
    )
    .await?;
    tx.commit().await?;
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
