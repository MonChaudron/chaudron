//! Recettes et leurs variantes par équipement : chaque variante en phases (mode rapide), chaque phase en gestes (mode
//! pas à pas, facultatifs). Quantités pour une portion standard, telles qu'achetées.
//! Publication idempotente par `slug` (PUT, ou import en lot par Claude).
//!
//! Deux niveaux : la recette **du foyer** (`household_id`), modifiée par ses comptes, et la recette **de la
//! communauté** (`household_id` nul), vue par tous les foyers et modifiée par l'admin de l'instance. Un slug de la
//! communauté n'existe dans aucun foyer : pour un foyer, un slug désigne une seule recette.
//!
//! Chaque élément (variante, phase, geste, ligne) a une clé stable, gardée d'une version à l'autre ; chaque
//! enregistrement qui change la recette en écrit une version (contenu complet), et revenir à une version la republie.

use std::collections::{HashMap, HashSet};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgConnection, PgPool};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    AppState,
    auth::{Author, Caller},
    error::{ApiError, ApiResult, ErrorBody},
    families::{self, Family},
    feedback::{self, Feedback, RatingCounts},
    ingredients::{self, NewIngredient},
    members,
    menus::{self, MenuInput, Warning},
    substitutions::{self, RecipeSubstitution},
    text::{code, like_patterns, search_key},
    types::{IngredientState, PortionRole, Scope, Unit},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/households/{id}/recipes", get(list_recipes))
        .route(
            "/households/{id}/recipes/{slug}",
            get(read_recipe).put(put_recipe).delete(delete_recipe),
        )
        .route(
            "/households/{id}/recipes/{slug}/versions",
            get(list_versions),
        )
        .route(
            "/households/{id}/recipes/{slug}/versions/{number}",
            get(read_version),
        )
        .route(
            "/households/{id}/recipes/{slug}/versions/{number}/restore",
            post(restore_version),
        )
        .route(
            "/community/recipes/{slug}",
            put(put_community).delete(delete_community),
        )
        .route("/households/{id}/import", post(import))
}

crate::types::text_enum! {
    /// Unité saisie : `kg`, `l` et `cl` sont convertis en `g` et `ml`.
    pub enum InputUnit {
        G = "g",
        Kg = "kg",
        Ml = "ml",
        Cl = "cl",
        L = "l",
        Piece = "piece",
        Tbsp = "tbsp",
        Tsp = "tsp",
        Pinch = "pinch",
    }
}

impl From<Unit> for InputUnit {
    fn from(unit: Unit) -> Self {
        match unit {
            Unit::G => Self::G,
            Unit::Ml => Self::Ml,
            Unit::Piece => Self::Piece,
            Unit::Tbsp => Self::Tbsp,
            Unit::Tsp => Self::Tsp,
            Unit::Pinch => Self::Pinch,
        }
    }
}

impl InputUnit {
    /// Unité normalisée et facteur de conversion.
    fn normalize(self) -> (Unit, f32) {
        match self {
            Self::G => (Unit::G, 1.0),
            Self::Kg => (Unit::G, 1000.0),
            Self::Ml => (Unit::Ml, 1.0),
            Self::Cl => (Unit::Ml, 10.0),
            Self::L => (Unit::Ml, 1000.0),
            Self::Piece => (Unit::Piece, 1.0),
            Self::Tbsp => (Unit::Tbsp, 1.0),
            Self::Tsp => (Unit::Tsp, 1.0),
            Self::Pinch => (Unit::Pinch, 1.0),
        }
    }
}

// Saisie

/// Ligne d'ingrédient saisie, pour une portion standard.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct LineInput {
    /// Clé stable, renvoyée par l'éditeur ; sans clé, celle de la ligne au même endroit, sinon une nouvelle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<Uuid>,
    /// Code de l'ingrédient du référentiel (`minced_beef`).
    pub ingredient: String,
    /// Forme d'achat ; par défaut, celle de l'ingrédient.
    pub state: Option<IngredientState>,
    /// Quantité telle qu'achetée ; absente : « au goût ».
    pub qty: Option<f32>,
    pub unit: InputUnit,
    pub role: PortionRole,
    #[serde(default)]
    pub optional: bool,
    /// Ne suit pas le nombre de portions (un citron, un cube de bouillon).
    #[serde(default)]
    pub per_batch: bool,
    pub note: Option<String>,
}

/// Geste du mode pas à pas.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct StepInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<Uuid>,
    pub text: String,
    /// Texte lu par Alexa, s'il diffère.
    pub spoken_text: Option<String>,
    pub timer_s: Option<i32>,
    /// Vidéo MP4 en HTTPS.
    pub video_url: Option<String>,
    /// Appareil du geste.
    pub equipment: Option<String>,
    /// Réglages pour les futurs connecteurs (vitesse, température, programme).
    #[serde(default)]
    #[schema(value_type = Object)]
    pub params: Option<Value>,
}

/// Phase du mode rapide.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct PhaseInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<Uuid>,
    pub title: String,
    pub text: String,
    pub spoken_text: Option<String>,
    pub timer_s: Option<i32>,
    /// -1 : la veille.
    #[serde(default)]
    pub day_offset: i16,
    #[serde(default)]
    pub steps: Vec<StepInput>,
}

/// Variante : une manière de cuisiner la recette.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct VariantInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<Uuid>,
    pub label: String,
    /// Codes du catalogue d'équipements.
    #[serde(default)]
    pub equipment: Vec<String>,
    pub active_min: i16,
    pub total_min: i16,
    /// Récipients à laver ; 1 par défaut.
    pub vessels: Option<i16>,
    /// Une seule variante par défaut ; sans indication, la première.
    #[serde(default)]
    pub is_default: bool,
    pub phases: Vec<PhaseInput>,
}

/// Recette complète, telle que publiée ; c'est aussi le contenu d'une version.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct RecipeInput {
    /// Obligatoire dans un lot ; dans un PUT, doit être celui de l'adresse s'il est donné.
    pub slug: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Mois de saison, de 1 à 12.
    #[serde(default)]
    pub season_months: Vec<i16>,
    pub source: Option<String>,
    pub ingredients: Vec<LineInput>,
    pub variants: Vec<VariantInput>,
    /// Un mot sur ce changement, gardé dans l'historique (pas dans la recette).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// Lecture

/// Ligne d'ingrédient, pour une portion standard.
#[derive(Serialize, ToSchema)]
pub struct RecipeLine {
    pub key: Uuid,
    /// Code de l'ingrédient.
    pub ingredient: String,
    pub ingredient_id: Uuid,
    pub name: String,
    pub state: IngredientState,
    /// Nulle : « au goût ».
    pub qty: Option<f32>,
    pub unit: Unit,
    pub role: PortionRole,
    pub optional: bool,
    pub per_batch: bool,
    pub note: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct Step {
    pub key: Uuid,
    pub text: String,
    pub spoken_text: Option<String>,
    pub timer_s: Option<i32>,
    pub video_url: Option<String>,
    pub equipment: Option<String>,
    #[schema(value_type = Object)]
    pub params: Value,
}

#[derive(Serialize, ToSchema)]
pub struct Phase {
    pub key: Uuid,
    pub title: String,
    pub text: String,
    pub spoken_text: Option<String>,
    pub timer_s: Option<i32>,
    pub day_offset: i16,
    /// Gestes du pas à pas ; vide : le pas à pas montre la phase.
    pub steps: Vec<Step>,
}

#[derive(Serialize, ToSchema)]
pub struct Variant {
    pub id: Uuid,
    pub key: Uuid,
    pub label: String,
    pub equipment: Vec<String>,
    /// Équipements de la variante que le foyer n'a pas.
    pub missing_equipment: Vec<String>,
    pub active_min: i16,
    pub total_min: i16,
    pub vessels: i16,
    pub is_default: bool,
    pub phases: Vec<Phase>,
}

/// Recette complète.
#[derive(Serialize, ToSchema)]
pub struct Recipe {
    pub slug: String,
    /// Recette de la communauté (modifiée par l'admin de l'instance), sinon du foyer.
    pub community: bool,
    /// Retirée du catalogue de la communauté : gardée par les menus qui l'utilisent.
    pub retired: bool,
    /// Numéro de la version courante (historique).
    pub version: i32,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    pub season_months: Vec<i16>,
    pub source: Option<String>,
    pub ingredients: Vec<RecipeLine>,
    /// La variante par défaut d'abord, puis dans l'ordre de publication.
    pub variants: Vec<Variant>,
    /// Les notes ouvertes du cahier du foyer qui lit la recette.
    pub notes: Vec<Feedback>,
    /// « Comment c'était ? », compté.
    pub ratings: RatingCounts,
    /// Copie d'une recette de la communauté : laquelle, et si elle a changé depuis.
    pub copied_from: Option<CopiedFrom>,
    /// La dernière proposition de cette recette à la communauté par le foyer qui la lit.
    pub submission: Option<SubmissionState>,
    /// Ses déclinaisons, et la préférée du foyer.
    pub family: Option<Family>,
    /// Les remplacements que la recette précise ou refuse.
    pub substitutions: Vec<RecipeSubstitution>,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// L'originale d'une copie (« d'après Hachis parmentier, v3 »).
#[derive(Serialize, ToSchema, FromRow)]
pub struct CopiedFrom {
    pub slug: String,
    pub title: String,
    /// La version de l'originale que la copie suit.
    pub version: i32,
    /// Version courante de l'originale.
    pub current_version: i32,
    /// L'originale a changé depuis.
    pub behind: bool,
}

/// Où en est la proposition d'une recette à la communauté.
#[derive(Serialize, ToSchema, FromRow)]
pub struct SubmissionState {
    /// `pending`, `accepted` ou `refused`.
    pub status: String,
    /// Le mot de l'admin.
    pub review_message: Option<String>,
}

/// Un élément d'une recette, désigné par sa clé.
pub(crate) enum Element<'a> {
    Line(&'a RecipeLine),
    /// Une variante : une note peut la viser, rien ne s'y change.
    Variant,
    Phase(&'a Phase),
    Step(&'a Step),
}

impl Recipe {
    /// L'élément de cette clé, s'il existe dans la version courante.
    pub(crate) fn element(&self, key: Uuid) -> Option<Element<'_>> {
        if let Some(line) = self.ingredients.iter().find(|l| l.key == key) {
            return Some(Element::Line(line));
        }
        for variant in &self.variants {
            if variant.key == key {
                return Some(Element::Variant);
            }
            for phase in &variant.phases {
                if phase.key == key {
                    return Some(Element::Phase(phase));
                }
                if let Some(step) = phase.steps.iter().find(|s| s.key == key) {
                    return Some(Element::Step(step));
                }
            }
        }
        None
    }

    /// La recette telle qu'on la publie (contenu d'une version), clés comprises.
    pub fn to_input(&self) -> RecipeInput {
        RecipeInput {
            slug: Some(self.slug.clone()),
            title: self.title.clone(),
            description: self.description.clone(),
            tags: self.tags.clone(),
            season_months: self.season_months.clone(),
            source: self.source.clone(),
            ingredients: self
                .ingredients
                .iter()
                .map(|l| LineInput {
                    key: Some(l.key),
                    ingredient: l.ingredient.clone(),
                    state: Some(l.state),
                    qty: l.qty,
                    unit: l.unit.into(),
                    role: l.role,
                    optional: l.optional,
                    per_batch: l.per_batch,
                    note: l.note.clone(),
                })
                .collect(),
            variants: self
                .variants
                .iter()
                .map(|v| VariantInput {
                    key: Some(v.key),
                    label: v.label.clone(),
                    equipment: v.equipment.clone(),
                    active_min: v.active_min,
                    total_min: v.total_min,
                    vessels: Some(v.vessels),
                    is_default: v.is_default,
                    phases: v
                        .phases
                        .iter()
                        .map(|p| PhaseInput {
                            key: Some(p.key),
                            title: p.title.clone(),
                            text: p.text.clone(),
                            spoken_text: p.spoken_text.clone(),
                            timer_s: p.timer_s,
                            day_offset: p.day_offset,
                            steps: p
                                .steps
                                .iter()
                                .map(|s| StepInput {
                                    key: Some(s.key),
                                    text: s.text.clone(),
                                    spoken_text: s.spoken_text.clone(),
                                    timer_s: s.timer_s,
                                    video_url: s.video_url.clone(),
                                    equipment: s.equipment.clone(),
                                    params: Some(s.params.clone()),
                                })
                                .collect(),
                        })
                        .collect(),
                })
                .collect(),
            message: None,
        }
    }
}

#[derive(FromRow)]
struct RecipeRow {
    id: Uuid,
    household_id: Option<Uuid>,
    family_id: Option<Uuid>,
    version: i32,
    retired: bool,
    slug: String,
    title: String,
    description: String,
    tags: Vec<String>,
    season_months: Vec<i16>,
    source: Option<String>,
    updated_at: OffsetDateTime,
}

#[derive(FromRow)]
struct LineRow {
    key: Uuid,
    code: String,
    ingredient_id: Uuid,
    name: String,
    #[sqlx(try_from = "String")]
    state: IngredientState,
    qty: Option<f32>,
    #[sqlx(try_from = "String")]
    unit: Unit,
    #[sqlx(try_from = "String")]
    role: PortionRole,
    optional: bool,
    per_batch: bool,
    note: Option<String>,
}

#[derive(FromRow)]
struct VariantRow {
    id: Uuid,
    key: Uuid,
    label: String,
    equipment_codes: Vec<String>,
    active_min: i16,
    total_min: i16,
    vessels: i16,
    is_default: bool,
}

#[derive(FromRow)]
struct PhaseRow {
    id: Uuid,
    key: Uuid,
    variant_id: Uuid,
    title: String,
    text: String,
    spoken_text: Option<String>,
    timer_s: Option<i32>,
    day_offset: i16,
}

#[derive(FromRow)]
struct StepRow {
    key: Uuid,
    phase_id: Uuid,
    text: String,
    spoken_text: Option<String>,
    timer_s: Option<i32>,
    video_url: Option<String>,
    equipment_code: Option<String>,
    params: Value,
}

/// La recette de ce slug que voit le foyer (la sienne, ou celle de la communauté) : identifiant, et foyer
/// propriétaire (`None` : la communauté).
pub(crate) async fn find(
    conn: &mut PgConnection,
    household_id: Uuid,
    slug: &str,
) -> ApiResult<Option<(Uuid, Option<Uuid>)>> {
    Ok(sqlx::query_as(
        "SELECT id, household_id FROM recipes WHERE (household_id = $1 OR household_id IS NULL) AND slug = $2
         ORDER BY household_id NULLS LAST LIMIT 1",
    )
    .bind(household_id)
    .bind(slug)
    .fetch_optional(conn)
    .await?)
}

/// La recette de ce slug, vue par ce foyer ; 404 s'il ne la voit pas.
pub(crate) async fn load(db: &PgPool, household_id: Uuid, slug: &str) -> ApiResult<Recipe> {
    let mut conn = db.acquire().await?;
    let (id, _) = find(&mut conn, household_id, slug)
        .await?
        .ok_or_else(ApiError::not_found)?;
    load_id(&mut conn, Some(household_id), id).await
}

/// La recette par son identifiant, vue par ce foyer (son matériel).
pub(crate) async fn load_by_id(db: &PgPool, household_id: Uuid, id: Uuid) -> ApiResult<Recipe> {
    let mut conn = db.acquire().await?;
    load_id(&mut conn, Some(household_id), id).await
}

/// La recette sans foyer qui la lit (l'admin, une version, une copie) : ni notes, ni matériel.
pub(crate) async fn load_plain(conn: &mut PgConnection, id: Uuid) -> ApiResult<Recipe> {
    load_id(conn, None, id).await
}

/// `viewer` : le foyer dont on compare le matériel (aucun : tout manque).
async fn load_id(conn: &mut PgConnection, viewer: Option<Uuid>, id: Uuid) -> ApiResult<Recipe> {
    let recipe: RecipeRow = sqlx::query_as(
        "SELECT id, household_id, family_id, version, retired_at IS NOT NULL AS retired, slug, title, description,
                tags, season_months, source, updated_at
         FROM recipes WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(ApiError::not_found)?;
    let lines: Vec<LineRow> = sqlx::query_as(
        "SELECT l.key, i.code, l.ingredient_id, i.name, l.state, l.qty, l.unit, l.role, l.optional, l.per_batch,
                l.note
         FROM recipe_ingredients l JOIN ingredients i ON i.id = l.ingredient_id
         WHERE l.recipe_id = $1 ORDER BY l.position",
    )
    .bind(recipe.id)
    .fetch_all(&mut *conn)
    .await?;
    let variants: Vec<VariantRow> = sqlx::query_as(
        "SELECT id, key, label, equipment_codes, active_min, total_min, vessels, is_default
         FROM recipe_variants WHERE recipe_id = $1 ORDER BY is_default DESC, position",
    )
    .bind(recipe.id)
    .fetch_all(&mut *conn)
    .await?;
    let phases: Vec<PhaseRow> = sqlx::query_as(
        "SELECT p.id, p.key, p.variant_id, p.title, p.text, p.spoken_text, p.timer_s, p.day_offset
         FROM variant_phases p JOIN recipe_variants v ON v.id = p.variant_id
         WHERE v.recipe_id = $1 ORDER BY p.position",
    )
    .bind(recipe.id)
    .fetch_all(&mut *conn)
    .await?;
    let steps: Vec<StepRow> = sqlx::query_as(
        "SELECT s.key, s.phase_id, s.text, s.spoken_text, s.timer_s, s.video_url, s.equipment_code, s.params
         FROM variant_steps s JOIN variant_phases p ON p.id = s.phase_id
         JOIN recipe_variants v ON v.id = p.variant_id
         WHERE v.recipe_id = $1 ORDER BY s.position",
    )
    .bind(recipe.id)
    .fetch_all(&mut *conn)
    .await?;
    let owned: HashSet<String> = sqlx::query_scalar(
        "SELECT equipment_code FROM household_equipment WHERE household_id = $1",
    )
    .bind(viewer)
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .collect();

    let mut steps_by_phase: HashMap<Uuid, Vec<Step>> = HashMap::new();
    for s in steps {
        steps_by_phase.entry(s.phase_id).or_default().push(Step {
            key: s.key,
            text: s.text,
            spoken_text: s.spoken_text,
            timer_s: s.timer_s,
            video_url: s.video_url,
            equipment: s.equipment_code,
            params: s.params,
        });
    }
    let mut phases_by_variant: HashMap<Uuid, Vec<Phase>> = HashMap::new();
    for p in phases {
        phases_by_variant
            .entry(p.variant_id)
            .or_default()
            .push(Phase {
                key: p.key,
                steps: steps_by_phase.remove(&p.id).unwrap_or_default(),
                title: p.title,
                text: p.text,
                spoken_text: p.spoken_text,
                timer_s: p.timer_s,
                day_offset: p.day_offset,
            });
    }
    let mut loaded = Recipe {
        slug: recipe.slug,
        community: recipe.household_id.is_none(),
        retired: recipe.retired,
        version: recipe.version,
        title: recipe.title,
        description: recipe.description,
        tags: recipe.tags,
        season_months: recipe.season_months,
        source: recipe.source,
        ingredients: lines
            .into_iter()
            .map(|l| RecipeLine {
                key: l.key,
                ingredient: l.code,
                ingredient_id: l.ingredient_id,
                name: l.name,
                state: l.state,
                qty: l.qty,
                unit: l.unit,
                role: l.role,
                optional: l.optional,
                per_batch: l.per_batch,
                note: l.note,
            })
            .collect(),
        variants: variants
            .into_iter()
            .map(|v| Variant {
                missing_equipment: v
                    .equipment_codes
                    .iter()
                    .filter(|c| !owned.contains(*c))
                    .cloned()
                    .collect(),
                phases: phases_by_variant.remove(&v.id).unwrap_or_default(),
                id: v.id,
                key: v.key,
                label: v.label,
                equipment: v.equipment_codes,
                active_min: v.active_min,
                total_min: v.total_min,
                vessels: v.vessels,
                is_default: v.is_default,
            })
            .collect(),
        notes: Vec::new(),
        ratings: RatingCounts::default(),
        copied_from: sqlx::query_as(
            "SELECT o.slug, o.title, c.copied_version AS version, o.version AS current_version,
                    o.version > c.copied_version AS behind
             FROM recipes c JOIN recipes o ON o.id = c.copied_from WHERE c.id = $1",
        )
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?,
        submission: None,
        family: match recipe.family_id {
            Some(family) => Some(families::family(conn, family, viewer).await?),
            None => None,
        },
        substitutions: substitutions::rules(conn, id).await?,
        updated_at: recipe.updated_at,
    };
    if let Some(household_id) = viewer {
        loaded.notes = feedback::open_notes(conn, household_id, id, &loaded).await?;
        loaded.ratings = feedback::counts(conn, id).await?;
        loaded.submission = sqlx::query_as(
            "SELECT status, review_message FROM recipe_submissions
             WHERE recipe_id = $1 AND household_id = $2 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(id)
        .bind(household_id)
        .fetch_optional(&mut *conn)
        .await?;
    }
    Ok(loaded)
}

// Écriture

pub(crate) fn check_slug(slug: &str) -> ApiResult<()> {
    let ok = !slug.is_empty()
        && slug.len() <= 80
        && slug.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        });
    if !ok {
        return Err(ApiError::invalid(
            "Slug invalide : minuscules, chiffres et tirets simples, 80 caractères au plus (« chili-con-carne »).",
        ));
    }
    Ok(())
}

fn text_field(value: &str, what: &str, max: usize) -> ApiResult<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max {
        return Err(ApiError::invalid(format!(
            "{what} doit faire entre 1 et {max} caractères."
        )));
    }
    Ok(value.to_owned())
}

fn optional_text(value: Option<&String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

/// Vérifie la recette (sans la base).
fn check(input: &RecipeInput) -> ApiResult<()> {
    text_field(&input.title, "Le titre", 120)?;
    if input.season_months.iter().any(|m| !(1..=12).contains(m)) {
        return Err(ApiError::invalid("Mois de saison entre 1 et 12."));
    }
    if input
        .message
        .as_deref()
        .is_some_and(|m| m.chars().count() > 300)
    {
        return Err(ApiError::invalid(
            "Le mot sur le changement fait 300 caractères au plus.",
        ));
    }
    if input
        .ingredients
        .iter()
        .any(|l| l.qty.is_some_and(|q| !(q > 0.0 && q.is_finite())))
    {
        return Err(ApiError::invalid(
            "La quantité doit être positive (sans quantité : au goût).",
        ));
    }
    if input.variants.is_empty() {
        return Err(ApiError::invalid("Au moins une variante."));
    }
    if input.variants.iter().filter(|v| v.is_default).count() > 1 {
        return Err(ApiError::invalid("Une seule variante par défaut."));
    }
    for v in &input.variants {
        text_field(&v.label, "Le nom de la variante", 80)?;
        if v.active_min < 0 || v.total_min < v.active_min {
            return Err(ApiError::invalid(format!(
                "{} : le temps actif ne peut pas dépasser le temps total.",
                v.label
            )));
        }
        if v.vessels.is_some_and(|n| n < 0) {
            return Err(ApiError::invalid("Nombre de récipients négatif."));
        }
        if v.phases.is_empty() {
            return Err(ApiError::invalid(format!(
                "{} : chaque variante a au moins une phase.",
                v.label
            )));
        }
        for p in &v.phases {
            text_field(&p.title, "Le titre d'une phase", 120)?;
            text_field(&p.text, "Le texte d'une phase", 4000)?;
            if !(-7..=0).contains(&p.day_offset) {
                return Err(ApiError::invalid(
                    "Une phase se fait le jour même ou jusqu'à 7 jours avant.",
                ));
            }
            let timers = std::iter::once(p.timer_s).chain(p.steps.iter().map(|s| s.timer_s));
            if timers.flatten().any(|t| t <= 0) {
                return Err(ApiError::invalid("Minuteur en secondes, positif."));
            }
            for s in &p.steps {
                text_field(&s.text, "Le texte d'un geste", 4000)?;
                if s.video_url
                    .as_deref()
                    .is_some_and(|u| !u.starts_with("https://"))
                {
                    return Err(ApiError::invalid("Vidéo en HTTPS seulement."));
                }
                if s.params.as_ref().is_some_and(|p| !p.is_object()) {
                    return Err(ApiError::invalid(
                        "Les réglages d'un geste sont un objet JSON.",
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Le slug d'une recette de la communauté n'existe dans aucun foyer, et inversement.
fn slug_taken(slug: &str) -> ApiError {
    ApiError::conflict(
        "slug_taken",
        format!("« {slug} » est une recette de la communauté : choisis un autre slug."),
    )
}

/// Clés des éléments de la version enregistrée, par leur place : un élément republié sans clé reprend la sienne.
#[derive(Default)]
struct OldKeys {
    /// Variante par son nom.
    variants: HashMap<String, Uuid>,
    /// Phase par (variante, position).
    phases: HashMap<(String, i16), Uuid>,
    /// Geste par (variante, phase, position).
    steps: HashMap<(String, i16, i16), Uuid>,
    /// Ligne par position : (ingrédient, clé).
    lines: Vec<(Uuid, Uuid)>,
}

impl OldKeys {
    async fn load(conn: &mut PgConnection, recipe_id: Uuid) -> ApiResult<Self> {
        let variants: Vec<(String, Uuid)> =
            sqlx::query_as("SELECT label, key FROM recipe_variants WHERE recipe_id = $1")
                .bind(recipe_id)
                .fetch_all(&mut *conn)
                .await?;
        let phases: Vec<(String, i16, Uuid)> = sqlx::query_as(
            "SELECT v.label, p.position, p.key FROM variant_phases p
             JOIN recipe_variants v ON v.id = p.variant_id WHERE v.recipe_id = $1",
        )
        .bind(recipe_id)
        .fetch_all(&mut *conn)
        .await?;
        let steps: Vec<(String, i16, i16, Uuid)> = sqlx::query_as(
            "SELECT v.label, p.position, s.position, s.key FROM variant_steps s
             JOIN variant_phases p ON p.id = s.phase_id JOIN recipe_variants v ON v.id = p.variant_id
             WHERE v.recipe_id = $1",
        )
        .bind(recipe_id)
        .fetch_all(&mut *conn)
        .await?;
        let lines: Vec<(Uuid, Uuid)> = sqlx::query_as(
            "SELECT ingredient_id, key FROM recipe_ingredients WHERE recipe_id = $1 ORDER BY position",
        )
        .bind(recipe_id)
        .fetch_all(&mut *conn)
        .await?;
        Ok(Self {
            variants: variants.into_iter().collect(),
            phases: phases.into_iter().map(|(v, p, k)| ((v, p), k)).collect(),
            steps: steps
                .into_iter()
                .map(|(v, p, s, k)| ((v, p, s), k))
                .collect(),
            lines,
        })
    }
}

/// Attribue les clés : celle donnée par l'éditeur, sinon celle de l'élément à la même place (si personne ne la
/// réclame), sinon une nouvelle. Jamais deux fois la même dans une recette.
struct Keys {
    claimed: HashSet<Uuid>,
    used: HashSet<Uuid>,
}

impl Keys {
    fn new(input: &RecipeInput) -> Self {
        let mut claimed: HashSet<Uuid> = input.ingredients.iter().filter_map(|l| l.key).collect();
        for v in &input.variants {
            claimed.extend(v.key);
            for p in &v.phases {
                claimed.extend(p.key);
                claimed.extend(p.steps.iter().filter_map(|s| s.key));
            }
        }
        Self {
            claimed,
            used: HashSet::new(),
        }
    }

    fn assign(&mut self, given: Option<Uuid>, by_place: Option<Uuid>) -> Uuid {
        let key = match (given, by_place) {
            (Some(key), _) if !self.used.contains(&key) => key,
            (None, Some(key)) if !self.used.contains(&key) && !self.claimed.contains(&key) => key,
            _ => Uuid::now_v7(),
        };
        self.used.insert(key);
        key
    }
}

/// Recette enregistrée : son identifiant, et si elle vient d'être créée.
pub(crate) struct Saved {
    pub id: Uuid,
    pub created: bool,
}

/// Enregistre la recette (création ou remplacement complet) et sa version si elle a changé. `owner` : le foyer, ou
/// `None` pour la communauté.
pub(crate) async fn save(
    conn: &mut PgConnection,
    owner: Option<Uuid>,
    slug: &str,
    input: &RecipeInput,
    author: &Author,
) -> ApiResult<Saved> {
    check_slug(slug)?;
    if input.slug.as_deref().is_some_and(|s| s != slug) {
        return Err(ApiError::invalid(
            "Le slug du corps diffère de celui de l'adresse.",
        ));
    }
    check(input)?;

    // Un slug à la fois : un foyer et la communauté ne prennent pas le même en même temps.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('recipe:' || $1))")
        .bind(slug)
        .execute(&mut *conn)
        .await?;
    let other_level: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM recipes WHERE slug = $1 AND (household_id IS NULL) = $2)",
    )
    .bind(slug)
    .bind(owner.is_some())
    .fetch_one(&mut *conn)
    .await?;
    if other_level {
        return Err(match owner {
            Some(_) => slug_taken(slug),
            None => ApiError::conflict(
                "slug_taken",
                format!("« {slug} » est déjà le slug d'une recette d'un foyer."),
            ),
        });
    }

    // Ingrédients du référentiel, par code.
    let codes: Vec<&str> = input
        .ingredients
        .iter()
        .map(|l| l.ingredient.as_str())
        .collect();
    let known: HashMap<String, (Uuid, String)> = sqlx::query_as::<_, (String, Uuid, String)>(
        "SELECT code, id, default_state FROM ingredients WHERE code = ANY($1)",
    )
    .bind(&codes)
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|(code, id, state)| (code, (id, state)))
    .collect();
    if let Some(unknown) = codes.iter().find(|c| !known.contains_key(**c)) {
        return Err(ApiError::invalid(format!(
            "Ingrédient inconnu : {unknown}."
        )));
    }
    // Équipements du catalogue.
    let mut equipment: Vec<&str> = input
        .variants
        .iter()
        .flat_map(|v| {
            v.equipment.iter().chain(
                v.phases
                    .iter()
                    .flat_map(|p| p.steps.iter().filter_map(|s| s.equipment.as_ref())),
            )
        })
        .map(String::as_str)
        .collect();
    equipment.sort_unstable();
    equipment.dedup();
    let catalog: Vec<String> =
        sqlx::query_scalar("SELECT code FROM equipment WHERE code = ANY($1)")
            .bind(&equipment)
            .fetch_all(&mut *conn)
            .await?;
    if let Some(unknown) = equipment.iter().find(|c| !catalog.iter().any(|k| k == *c)) {
        return Err(ApiError::invalid(format!(
            "Équipement inconnu : {unknown}."
        )));
    }

    let title = text_field(&input.title, "Le titre", 120)?;
    let mut tags: Vec<String> = input
        .tags
        .iter()
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .collect();
    tags.dedup();
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM recipes WHERE slug = $1 AND household_id IS NOT DISTINCT FROM $2",
    )
    .bind(slug)
    .bind(owner)
    .fetch_optional(&mut *conn)
    .await?;
    let old = match existing {
        Some(id) => OldKeys::load(conn, id).await?,
        None => OldKeys::default(),
    };
    let recipe_id = match existing {
        Some(id) => {
            sqlx::query(
                "UPDATE recipes SET title = $2, search_key = $3, description = $4, tags = $5, season_months = $6,
                        source = $7, updated_at = now()
                 WHERE id = $1",
            )
            .bind(id)
            .bind(&title)
            .bind(search_key(&title))
            .bind(input.description.trim())
            .bind(&tags)
            .bind(&input.season_months)
            .bind(optional_text(input.source.as_ref()))
            .execute(&mut *conn)
            .await?;
            id
        }
        None => {
            let id = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO recipes (id, household_id, slug, title, search_key, description, tags, season_months,
                                      source)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            )
            .bind(id)
            .bind(owner)
            .bind(slug)
            .bind(&title)
            .bind(search_key(&title))
            .bind(input.description.trim())
            .bind(&tags)
            .bind(&input.season_months)
            .bind(optional_text(input.source.as_ref()))
            .execute(&mut *conn)
            .await?;
            id
        }
    };

    sqlx::query("DELETE FROM recipe_ingredients WHERE recipe_id = $1")
        .bind(recipe_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM recipe_variants WHERE recipe_id = $1")
        .bind(recipe_id)
        .execute(&mut *conn)
        .await?;

    let mut keys = Keys::new(input);
    let mut old_lines: Vec<Option<(Uuid, Uuid)>> = old.lines.iter().copied().map(Some).collect();
    for (position, line) in input.ingredients.iter().enumerate() {
        let (ingredient_id, default_state) = &known[&line.ingredient];
        // Sans clé : la ligne à la même place si c'est le même ingrédient, sinon la première de cet ingrédient.
        let by_place = match old_lines.get(position).copied().flatten() {
            Some((ingredient, key)) if ingredient == *ingredient_id => {
                old_lines[position] = None;
                Some(key)
            }
            _ => old_lines
                .iter_mut()
                .find(|l| l.is_some_and(|(ingredient, _)| ingredient == *ingredient_id))
                .and_then(Option::take)
                .map(|(_, key)| key),
        };
        let key = keys.assign(line.key, by_place);
        let (unit, factor) = line.unit.normalize();
        let state = line
            .state
            .map(|s| s.as_str().to_owned())
            .unwrap_or_else(|| default_state.clone());
        sqlx::query(
            "INSERT INTO recipe_ingredients
                (recipe_id, position, ingredient_id, state, qty, unit, role, optional, per_batch, note, key)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(recipe_id)
        .bind(position as i16)
        .bind(ingredient_id)
        .bind(state)
        .bind(line.qty.map(|q| q * factor))
        .bind(unit.as_str())
        .bind(line.role.as_str())
        .bind(line.optional)
        .bind(line.per_batch)
        .bind(optional_text(line.note.as_ref()))
        .bind(key)
        .execute(&mut *conn)
        .await?;
    }

    let default = input
        .variants
        .iter()
        .position(|v| v.is_default)
        .unwrap_or(0);
    for (position, variant) in input.variants.iter().enumerate() {
        let variant_id = Uuid::now_v7();
        let label = variant.label.trim();
        let key = keys.assign(variant.key, old.variants.get(label).copied());
        sqlx::query(
            "INSERT INTO recipe_variants
                (id, recipe_id, position, label, equipment_codes, active_min, total_min, vessels, is_default, key)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(variant_id)
        .bind(recipe_id)
        .bind(position as i16)
        .bind(label)
        .bind(&variant.equipment)
        .bind(variant.active_min)
        .bind(variant.total_min)
        .bind(variant.vessels.unwrap_or(1))
        .bind(position == default)
        .bind(key)
        .execute(&mut *conn)
        .await?;
        for (position, phase) in variant.phases.iter().enumerate() {
            let phase_id = Uuid::now_v7();
            let place = (label.to_owned(), position as i16);
            let key = keys.assign(phase.key, old.phases.get(&place).copied());
            sqlx::query(
                "INSERT INTO variant_phases (id, variant_id, position, title, text, spoken_text, timer_s, day_offset,
                                             key)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            )
            .bind(phase_id)
            .bind(variant_id)
            .bind(position as i16)
            .bind(phase.title.trim())
            .bind(phase.text.trim())
            .bind(optional_text(phase.spoken_text.as_ref()))
            .bind(phase.timer_s)
            .bind(phase.day_offset)
            .bind(key)
            .execute(&mut *conn)
            .await?;
            for (step_position, step) in phase.steps.iter().enumerate() {
                let place = (place.0.clone(), place.1, step_position as i16);
                let key = keys.assign(step.key, old.steps.get(&place).copied());
                sqlx::query(
                    "INSERT INTO variant_steps
                        (id, phase_id, position, text, spoken_text, timer_s, video_url, equipment_code, params, key)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
                )
                .bind(Uuid::now_v7())
                .bind(phase_id)
                .bind(step_position as i16)
                .bind(step.text.trim())
                .bind(optional_text(step.spoken_text.as_ref()))
                .bind(step.timer_s)
                .bind(&step.video_url)
                .bind(&step.equipment)
                .bind(step.params.clone().unwrap_or_else(|| Value::Object(Default::default())))
                .bind(key)
                .execute(&mut *conn)
                .await?;
            }
        }
    }
    let message = optional_text(input.message.as_ref()).unwrap_or_default();
    record_version(conn, recipe_id, author, &message, false).await?;
    Ok(Saved {
        id: recipe_id,
        created: existing.is_none(),
    })
}

/// Écrit une version si le contenu a changé depuis la dernière (ou toujours, `force` : un changement de niveau) ;
/// renvoie le numéro de la version courante.
pub(crate) async fn record_version(
    conn: &mut PgConnection,
    recipe_id: Uuid,
    author: &Author,
    message: &str,
    force: bool,
) -> ApiResult<i32> {
    let recipe = load_id(conn, None, recipe_id).await?;
    let content = serde_json::to_value(recipe.to_input()).map_err(|_| ApiError::internal())?;
    let last: Option<Value> = sqlx::query_scalar(
        "SELECT content FROM recipe_versions WHERE recipe_id = $1 ORDER BY number DESC LIMIT 1",
    )
    .bind(recipe_id)
    .fetch_optional(&mut *conn)
    .await?;
    if !force && last.as_ref() == Some(&content) {
        return Ok(recipe.version);
    }
    let number = recipe.version + 1;
    sqlx::query(
        "INSERT INTO recipe_versions (recipe_id, number, content, author_id, author_name, message)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(recipe_id)
    .bind(number)
    .bind(&content)
    .bind(author.id)
    .bind(&author.name)
    .bind(message)
    .execute(&mut *conn)
    .await?;
    sqlx::query("UPDATE recipes SET version = $2 WHERE id = $1")
        .bind(recipe_id)
        .bind(number)
        .execute(&mut *conn)
        .await?;
    Ok(number)
}

/// Version de départ des recettes d'avant l'historique (au démarrage) ; renvoie combien ont été écrites.
pub async fn backfill_versions(db: &PgPool) -> ApiResult<usize> {
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM recipes WHERE version = 0")
        .fetch_all(db)
        .await?;
    let author = Author {
        id: None,
        name: "Chaudron".into(),
    };
    for id in &ids {
        let mut tx = db.begin().await?;
        record_version(&mut tx, *id, &author, "Version de départ", false).await?;
        tx.commit().await?;
    }
    Ok(ids.len())
}

// Routes

/// Variante par défaut, dans la liste des recettes.
#[derive(Serialize, ToSchema, FromRow)]
pub struct VariantSummary {
    pub label: String,
    pub equipment: Vec<String>,
    pub active_min: i16,
    pub total_min: i16,
    pub vessels: i16,
}

/// Recette, dans la liste du foyer.
#[derive(Serialize, ToSchema)]
pub struct RecipeSummary {
    pub slug: String,
    pub title: String,
    /// Recette de la communauté.
    pub community: bool,
    /// Sa famille de déclinaisons.
    pub family_id: Option<Uuid>,
    /// La préférée du foyer dans sa famille.
    pub preferred: bool,
    pub description: String,
    pub tags: Vec<String>,
    pub season_months: Vec<i16>,
    pub default_variant: VariantSummary,
    pub variant_count: i64,
}

#[derive(FromRow)]
struct SummaryRow {
    slug: String,
    title: String,
    community: bool,
    family_id: Option<Uuid>,
    preferred: bool,
    description: String,
    tags: Vec<String>,
    season_months: Vec<i16>,
    variant_count: i64,
    #[sqlx(flatten)]
    default_variant: VariantSummary,
}

crate::types::text_enum! {
    /// Niveau d'une recette.
    pub enum RecipeScope {
        /// Les recettes du foyer.
        Household = "household",
        /// Les recettes de la communauté.
        Community = "community",
    }
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RecipeQuery {
    /// Mots cherchés dans le titre et les ingrédients, sans tenir compte des accents ni de la casse.
    pub q: Option<String>,
    /// Étiquette exacte.
    pub tag: Option<String>,
    /// Un seul niveau ; sans, les deux.
    #[param(inline)]
    pub scope: Option<RecipeScope>,
}

/// Recettes du foyer et de la communauté (sauf celles qui sont retirées), par titre.
#[utoipa::path(
    get,
    path = "/api/v1/households/{id}/recipes",
    tag = "recettes",
    params(("id" = Uuid, Path), RecipeQuery),
    responses(
        (status = 200, body = Vec<RecipeSummary>),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn list_recipes(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Query(query): Query<RecipeQuery>,
) -> ApiResult<Json<Vec<RecipeSummary>>> {
    caller.require(&state.db, id, Scope::Read).await?;
    let patterns = like_patterns(query.q.as_deref().unwrap_or(""));
    let rows: Vec<SummaryRow> = sqlx::query_as(
        "SELECT r.slug, r.title, r.household_id IS NULL AS community, r.family_id,
                EXISTS (SELECT 1 FROM family_preferences p WHERE p.household_id = $1 AND p.recipe_id = r.id)
                    AS preferred,
                r.description, r.tags, r.season_months,
                (SELECT count(*) FROM recipe_variants c WHERE c.recipe_id = r.id) AS variant_count,
                d.label, d.equipment_codes AS equipment, d.active_min, d.total_min, d.vessels
         FROM recipes r JOIN recipe_variants d ON d.recipe_id = r.id AND d.is_default
         WHERE (r.household_id = $1 OR (r.household_id IS NULL AND r.retired_at IS NULL))
           AND ($4::text IS NULL OR ($4 = 'community') = (r.household_id IS NULL))
           AND ($2::text IS NULL OR $2 = ANY(r.tags))
           AND r.search_key || ' ' || coalesce((
                 SELECT string_agg(i.search_key, ' ')
                 FROM recipe_ingredients l JOIN ingredients i ON i.id = l.ingredient_id
                 WHERE l.recipe_id = r.id), '') LIKE ALL($3)
         ORDER BY r.search_key",
    )
    .bind(id)
    .bind(query.tag.as_deref().map(str::trim))
    .bind(&patterns)
    .bind(query.scope.map(RecipeScope::as_str))
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| RecipeSummary {
                slug: r.slug,
                title: r.title,
                community: r.community,
                family_id: r.family_id,
                preferred: r.preferred,
                description: r.description,
                tags: r.tags,
                season_months: r.season_months,
                default_variant: r.default_variant,
                variant_count: r.variant_count,
            })
            .collect(),
    ))
}

/// Recette complète, avec les équipements qui manquent au foyer pour chaque variante.
#[utoipa::path(
    get,
    path = "/api/v1/households/{id}/recipes/{slug}",
    tag = "recettes",
    params(("id" = Uuid, Path), ("slug" = String, Path)),
    responses(
        (status = 200, body = Recipe),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn read_recipe(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, slug)): Path<(Uuid, String)>,
) -> ApiResult<Json<Recipe>> {
    caller.require(&state.db, id, Scope::Read).await?;
    load(&state.db, id, &slug).await.map(Json)
}

/// Publie une recette du foyer : la crée (201) ou la remplace entièrement (200). Idempotent ; une version s'écrit
/// si le contenu change. Le slug d'une recette de la communauté est refusé (409 `slug_taken`).
#[utoipa::path(
    put,
    path = "/api/v1/households/{id}/recipes/{slug}",
    tag = "recettes",
    params(("id" = Uuid, Path), ("slug" = String, Path)),
    request_body = RecipeInput,
    responses(
        (status = 200, description = "Remplacée", body = Recipe),
        (status = 201, description = "Créée", body = Recipe),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, description = "Slug d'une recette de la communauté", body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn put_recipe(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, slug)): Path<(Uuid, String)>,
    Json(body): Json<RecipeInput>,
) -> ApiResult<(StatusCode, Json<Recipe>)> {
    caller.require(&state.db, id, Scope::RecipesWrite).await?;
    let author = caller.author(&state.db).await?;
    let mut tx = state.db.begin().await?;
    let saved = save(&mut tx, Some(id), &slug, &body, &author).await?;
    tx.commit().await?;
    let status = if saved.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(load_by_id(&state.db, id, saved.id).await?)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/households/{id}/recipes/{slug}",
    tag = "recettes",
    params(("id" = Uuid, Path), ("slug" = String, Path)),
    responses(
        (status = 204),
        (status = 403, description = "Recette de la communauté", body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, description = "Prévue dans un menu", body = ErrorBody),
    )
)]
pub async fn delete_recipe(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, slug)): Path<(Uuid, String)>,
) -> ApiResult<StatusCode> {
    caller.require(&state.db, id, Scope::RecipesWrite).await?;
    let mut conn = state.db.acquire().await?;
    let (recipe_id, owner) = find(&mut conn, id, &slug)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if owner.is_none() {
        return Err(ApiError::forbidden(
            "Recette de la communauté : seul l'admin de l'instance la retire.",
        ));
    }
    let in_menu: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM menu_slots WHERE recipe_id = $1)")
            .bind(recipe_id)
            .fetch_one(&mut *conn)
            .await?;
    if in_menu {
        return Err(ApiError::conflict(
            "recipe_in_menu",
            "Cette recette est prévue dans un menu : retirez-la du menu d'abord.",
        ));
    }
    sqlx::query("DELETE FROM recipes WHERE id = $1")
        .bind(recipe_id)
        .execute(&mut *conn)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Modifie une recette de la communauté (admin de l'instance). Elle se crée en acceptant une recette proposée.
#[utoipa::path(
    put,
    path = "/api/v1/community/recipes/{slug}",
    tag = "communauté",
    params(("slug" = String, Path)),
    request_body = RecipeInput,
    responses(
        (status = 200, body = Recipe),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn put_community(
    State(state): State<AppState>,
    caller: Caller,
    Path(slug): Path<String>,
    Json(body): Json<RecipeInput>,
) -> ApiResult<Json<Recipe>> {
    caller.require_admin(&state).await?;
    let author = caller.author(&state.db).await?;
    let mut tx = state.db.begin().await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM recipes WHERE household_id IS NULL AND slug = $1)",
    )
    .bind(&slug)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(ApiError::not_found());
    }
    let saved = save(&mut tx, None, &slug, &body, &author).await?;
    let recipe = load_id(&mut tx, None, saved.id).await?;
    tx.commit().await?;
    Ok(Json(recipe))
}

/// Supprime une recette de la communauté (admin) ; utilisée par un menu ou une cuisson, elle est seulement retirée
/// du catalogue, et ceux qui l'utilisent la gardent.
#[utoipa::path(
    delete,
    path = "/api/v1/community/recipes/{slug}",
    tag = "communauté",
    params(("slug" = String, Path)),
    responses(
        (status = 204),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn delete_community(
    State(state): State<AppState>,
    caller: Caller,
    Path(slug): Path<String>,
) -> ApiResult<StatusCode> {
    caller.require_admin(&state).await?;
    let id: Uuid =
        sqlx::query_scalar("SELECT id FROM recipes WHERE household_id IS NULL AND slug = $1")
            .bind(&slug)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(ApiError::not_found)?;
    let used: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM menu_slots WHERE recipe_id = $1)
             OR EXISTS (SELECT 1 FROM cooking_sessions WHERE recipe_id = $1)",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    let query = if used {
        "UPDATE recipes SET retired_at = coalesce(retired_at, now()) WHERE id = $1"
    } else {
        "DELETE FROM recipes WHERE id = $1"
    };
    sqlx::query(query).bind(id).execute(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Une version de l'historique, sans son contenu.
#[derive(Serialize, ToSchema, FromRow)]
pub struct VersionInfo {
    pub number: i32,
    pub author_name: String,
    /// Le mot laissé avec le changement ; vide s'il n'y en a pas.
    pub message: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// Une version avec son contenu complet.
#[derive(Serialize, ToSchema)]
pub struct RecipeVersion {
    pub number: i32,
    pub author_name: String,
    pub message: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub content: RecipeInput,
}

/// Historique d'une recette, de la plus récente à la plus ancienne.
#[utoipa::path(
    get,
    path = "/api/v1/households/{id}/recipes/{slug}/versions",
    tag = "recettes",
    params(("id" = Uuid, Path), ("slug" = String, Path)),
    responses(
        (status = 200, body = Vec<VersionInfo>),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn list_versions(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, slug)): Path<(Uuid, String)>,
) -> ApiResult<Json<Vec<VersionInfo>>> {
    caller.require(&state.db, id, Scope::Read).await?;
    let mut conn = state.db.acquire().await?;
    let (recipe_id, _) = find(&mut conn, id, &slug)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let versions = sqlx::query_as(
        "SELECT number, author_name, message, created_at FROM recipe_versions
         WHERE recipe_id = $1 ORDER BY number DESC",
    )
    .bind(recipe_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(Json(versions))
}

async fn version(
    conn: &mut PgConnection,
    recipe_id: Uuid,
    number: i32,
) -> ApiResult<RecipeVersion> {
    let row: (i32, String, String, OffsetDateTime, Value) = sqlx::query_as(
        "SELECT number, author_name, message, created_at, content FROM recipe_versions
         WHERE recipe_id = $1 AND number = $2",
    )
    .bind(recipe_id)
    .bind(number)
    .fetch_optional(conn)
    .await?
    .ok_or_else(ApiError::not_found)?;
    Ok(RecipeVersion {
        number: row.0,
        author_name: row.1,
        message: row.2,
        created_at: row.3,
        content: serde_json::from_value(row.4).map_err(|_| ApiError::internal())?,
    })
}

/// Une version, avec la recette telle qu'elle était.
#[utoipa::path(
    get,
    path = "/api/v1/households/{id}/recipes/{slug}/versions/{number}",
    tag = "recettes",
    params(("id" = Uuid, Path), ("slug" = String, Path), ("number" = i32, Path)),
    responses(
        (status = 200, body = RecipeVersion),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn read_version(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, slug, number)): Path<(Uuid, String, i32)>,
) -> ApiResult<Json<RecipeVersion>> {
    caller.require(&state.db, id, Scope::Read).await?;
    let mut conn = state.db.acquire().await?;
    let (recipe_id, _) = find(&mut conn, id, &slug)
        .await?
        .ok_or_else(ApiError::not_found)?;
    version(&mut conn, recipe_id, number).await.map(Json)
}

/// Revient à une version : la republie, ce qui crée une nouvelle version (rien ne se perd). Recette du foyer : tout
/// compte du foyer ; de la communauté : l'admin de l'instance.
#[utoipa::path(
    post,
    path = "/api/v1/households/{id}/recipes/{slug}/versions/{number}/restore",
    tag = "recettes",
    params(("id" = Uuid, Path), ("slug" = String, Path), ("number" = i32, Path)),
    responses(
        (status = 200, body = Recipe),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn restore_version(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, slug, number)): Path<(Uuid, String, i32)>,
) -> ApiResult<Json<Recipe>> {
    caller.require(&state.db, id, Scope::Read).await?;
    let mut tx = state.db.begin().await?;
    let (recipe_id, owner) = find(&mut tx, id, &slug)
        .await?
        .ok_or_else(ApiError::not_found)?;
    match owner {
        Some(_) => caller.require(&state.db, id, Scope::RecipesWrite).await?,
        None => {
            caller.require_admin(&state).await?;
        }
    }
    let mut content = version(&mut tx, recipe_id, number).await?.content;
    content.slug = None;
    content.message = Some(format!("Retour à la version {number}"));
    let author = caller.author(&state.db).await?;
    save(&mut tx, owner, &slug, &content, &author).await?;
    tx.commit().await?;
    load_by_id(&state.db, id, recipe_id).await.map(Json)
}

/// Lot publié par Claude : ingrédients à ajouter au référentiel, recettes, et le menu de la semaine.
#[derive(Deserialize, ToSchema)]
pub struct Lot {
    /// Ingrédients nouveaux ; ceux qui existent déjà (même nom, aux accents près) sont ignorés.
    #[serde(default)]
    pub ingredients: Vec<NewIngredient>,
    /// Chaque recette avec son `slug`.
    #[serde(default)]
    pub recipes: Vec<RecipeInput>,
    /// Menu de la semaine (`week_start` obligatoire) : remplace le menu de même `week_start`.
    pub menu: Option<MenuInput>,
}

/// Recette du lot laissée de côté.
#[derive(Serialize, ToSchema)]
pub struct RecipeWarning {
    pub slug: String,
    /// `slug_taken` : le slug est celui d'une recette de la communauté, que le menu du lot utilise à la place.
    pub code: String,
    pub message: String,
}

/// Une recette créée par l'import, et celles qui lui ressemblent.
#[derive(Serialize, ToSchema)]
pub struct ImportSimilar {
    pub slug: String,
    pub similar_to: Vec<String>,
}

/// Ce que l'import a fait.
#[derive(Serialize, ToSchema)]
pub struct ImportResult {
    /// Codes des ingrédients créés.
    pub ingredients_created: Vec<String>,
    pub recipes_created: Vec<String>,
    pub recipes_updated: Vec<String>,
    /// Recettes du lot laissées de côté.
    pub recipe_warnings: Vec<RecipeWarning>,
    /// Recettes créées qui ressemblent à d'autres (rien n'en est fait : à l'auteur de décider).
    pub similar: Vec<ImportSimilar>,
    /// Le lot avait un menu, et il est publié.
    pub menu_imported: bool,
    /// Avertissements du menu publié : conflits d'appareil, matériel absent, exclusions…
    pub warnings: Vec<Warning>,
}

/// Importe un lot en une transaction : tout ou rien. Idempotent (clé : nom des ingrédients, `slug` des recettes,
/// `week_start` du menu). Recettes et ingrédients demandent `recipes:write`, le menu `menus:write`. Une recette au
/// slug d'une recette de la communauté est laissée de côté (`recipe_warnings`) : le menu prend celle de la
/// communauté.
#[utoipa::path(
    post,
    path = "/api/v1/households/{id}/import",
    tag = "recettes",
    params(("id" = Uuid, Path)),
    request_body = Lot,
    responses(
        (status = 200, body = ImportResult),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 422, description = "Lot refusé, rien n'est écrit", body = ErrorBody),
    )
)]
pub async fn import(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Json(lot): Json<Lot>,
) -> ApiResult<Json<ImportResult>> {
    if lot.menu.is_none() || !lot.ingredients.is_empty() || !lot.recipes.is_empty() {
        caller.require(&state.db, id, Scope::RecipesWrite).await?;
    }
    let menu_start = match &lot.menu {
        Some(menu) => {
            caller.require(&state.db, id, Scope::MenusWrite).await?;
            Some(menu.week_start.ok_or_else(|| {
                ApiError::invalid("Le menu du lot a une date de début (`week_start`).")
            })?)
        }
        None => None,
    };
    let author = caller.author(&state.db).await?;
    let mut result = ImportResult {
        ingredients_created: Vec::new(),
        recipes_created: Vec::new(),
        recipes_updated: Vec::new(),
        recipe_warnings: Vec::new(),
        similar: Vec::new(),
        menu_imported: false,
        warnings: Vec::new(),
    };
    let mut tx = state.db.begin().await?;
    for ingredient in &lot.ingredients {
        let name = ingredient.name.trim();
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM ingredients WHERE search_key = $1 OR code = $2)",
        )
        .bind(search_key(name))
        .bind(code(name))
        .fetch_one(&mut *tx)
        .await?;
        if !exists {
            ingredients::insert(&mut tx, ingredient)
                .await
                .map_err(|e| e.context(name))?;
            result.ingredients_created.push(code(name));
        } else if let Some(days) = ingredient.keeps_days {
            // Un ingrédient connu : le référentiel est commun à l'instance, le lot complète une durée absente
            // (le saumon fumé), il ne remplace jamais celle qui existe.
            if !(1..=60).contains(&days) {
                return Err(ApiError::invalid(format!("{name} : de 1 à 60 jours.")));
            }
            sqlx::query(
                "UPDATE ingredients SET keeps_days = $2 WHERE code = $1 AND keeps_days IS NULL",
            )
            .bind(code(name))
            .bind(days)
            .execute(&mut *tx)
            .await?;
        }
    }
    for recipe in &lot.recipes {
        let Some(slug) = recipe.slug.as_deref() else {
            return Err(ApiError::invalid(format!(
                "{} : chaque recette du lot a un slug.",
                recipe.title
            )));
        };
        let community: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM recipes WHERE household_id IS NULL AND slug = $1)",
        )
        .bind(slug)
        .fetch_one(&mut *tx)
        .await?;
        if community {
            result.recipe_warnings.push(RecipeWarning {
                slug: slug.to_owned(),
                code: "slug_taken".into(),
                message: format!(
                    "{} : laissée de côté, « {slug} » est une recette de la communauté (le menu prend la sienne).",
                    recipe.title
                ),
            });
            continue;
        }
        let saved = save(&mut tx, Some(id), slug, recipe, &author)
            .await
            .map_err(|e| e.context(slug))?;
        if saved.created {
            let close = families::similar_to(&mut tx, id, recipe, Some(slug)).await?;
            if !close.is_empty() {
                result.similar.push(ImportSimilar {
                    slug: slug.to_owned(),
                    similar_to: close.into_iter().map(|s| s.slug).collect(),
                });
            }
            result.recipes_created.push(slug.to_owned());
        } else {
            result.recipes_updated.push(slug.to_owned());
        }
    }
    if let (Some(menu), Some(week_start)) = (&lot.menu, menu_start) {
        let members = members::load(&state.db, id, None).await?;
        menus::save(&mut tx, &members, id, week_start, menu)
            .await
            .map_err(|e| e.context(&format!("menu du {week_start}")))?;
        result.menu_imported = true;
    }
    tx.commit().await?;
    if let Some(week_start) = menu_start {
        state.notify(id, "menu", Some(week_start), None);
        result.warnings = menus::read(&state.db, id, week_start).await?.warnings;
    }
    Ok(Json(result))
}
