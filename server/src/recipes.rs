//! Recettes du foyer et leurs variantes par équipement : chaque variante en phases (mode rapide), chaque phase en
//! gestes (mode pas à pas, facultatifs). Quantités pour une portion standard, telles qu'achetées.
//! Publication idempotente par `slug` (PUT, ou import en lot par Claude).

use std::collections::{HashMap, HashSet};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgConnection, PgPool};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    AppState,
    auth::Caller,
    error::{ApiError, ApiResult, ErrorBody},
    ingredients::{self, NewIngredient},
    members,
    menus::{self, MenuInput, Warning},
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
#[derive(Deserialize, ToSchema)]
pub struct LineInput {
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
#[derive(Deserialize, ToSchema)]
pub struct StepInput {
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
#[derive(Deserialize, ToSchema)]
pub struct PhaseInput {
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
#[derive(Deserialize, ToSchema)]
pub struct VariantInput {
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

/// Recette complète, telle que publiée.
#[derive(Deserialize, ToSchema)]
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
}

// Lecture

/// Ligne d'ingrédient, pour une portion standard.
#[derive(Serialize, ToSchema)]
pub struct RecipeLine {
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
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    pub season_months: Vec<i16>,
    pub source: Option<String>,
    pub ingredients: Vec<RecipeLine>,
    /// La variante par défaut d'abord, puis dans l'ordre de publication.
    pub variants: Vec<Variant>,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(FromRow)]
struct RecipeRow {
    id: Uuid,
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
    variant_id: Uuid,
    title: String,
    text: String,
    spoken_text: Option<String>,
    timer_s: Option<i32>,
    day_offset: i16,
}

#[derive(FromRow)]
struct StepRow {
    phase_id: Uuid,
    text: String,
    spoken_text: Option<String>,
    timer_s: Option<i32>,
    video_url: Option<String>,
    equipment_code: Option<String>,
    params: Value,
}

pub(crate) async fn load(db: &PgPool, household_id: Uuid, slug: &str) -> ApiResult<Recipe> {
    let recipe: RecipeRow = sqlx::query_as(
        "SELECT id, slug, title, description, tags, season_months, source, updated_at
         FROM recipes WHERE household_id = $1 AND slug = $2",
    )
    .bind(household_id)
    .bind(slug)
    .fetch_optional(db)
    .await?
    .ok_or_else(ApiError::not_found)?;
    let lines: Vec<LineRow> = sqlx::query_as(
        "SELECT i.code, l.ingredient_id, i.name, l.state, l.qty, l.unit, l.role, l.optional, l.per_batch, l.note
         FROM recipe_ingredients l JOIN ingredients i ON i.id = l.ingredient_id
         WHERE l.recipe_id = $1 ORDER BY l.position",
    )
    .bind(recipe.id)
    .fetch_all(db)
    .await?;
    let variants: Vec<VariantRow> = sqlx::query_as(
        "SELECT id, label, equipment_codes, active_min, total_min, vessels, is_default
         FROM recipe_variants WHERE recipe_id = $1 ORDER BY is_default DESC, position",
    )
    .bind(recipe.id)
    .fetch_all(db)
    .await?;
    let phases: Vec<PhaseRow> = sqlx::query_as(
        "SELECT p.id, p.variant_id, p.title, p.text, p.spoken_text, p.timer_s, p.day_offset
         FROM variant_phases p JOIN recipe_variants v ON v.id = p.variant_id
         WHERE v.recipe_id = $1 ORDER BY p.position",
    )
    .bind(recipe.id)
    .fetch_all(db)
    .await?;
    let steps: Vec<StepRow> = sqlx::query_as(
        "SELECT s.phase_id, s.text, s.spoken_text, s.timer_s, s.video_url, s.equipment_code, s.params
         FROM variant_steps s JOIN variant_phases p ON p.id = s.phase_id
         JOIN recipe_variants v ON v.id = p.variant_id
         WHERE v.recipe_id = $1 ORDER BY s.position",
    )
    .bind(recipe.id)
    .fetch_all(db)
    .await?;
    let owned: HashSet<String> = sqlx::query_scalar(
        "SELECT equipment_code FROM household_equipment WHERE household_id = $1",
    )
    .bind(household_id)
    .fetch_all(db)
    .await?
    .into_iter()
    .collect();

    let mut steps_by_phase: HashMap<Uuid, Vec<Step>> = HashMap::new();
    for s in steps {
        steps_by_phase.entry(s.phase_id).or_default().push(Step {
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
                steps: steps_by_phase.remove(&p.id).unwrap_or_default(),
                title: p.title,
                text: p.text,
                spoken_text: p.spoken_text,
                timer_s: p.timer_s,
                day_offset: p.day_offset,
            });
    }
    Ok(Recipe {
        slug: recipe.slug,
        title: recipe.title,
        description: recipe.description,
        tags: recipe.tags,
        season_months: recipe.season_months,
        source: recipe.source,
        ingredients: lines
            .into_iter()
            .map(|l| RecipeLine {
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
                label: v.label,
                equipment: v.equipment_codes,
                active_min: v.active_min,
                total_min: v.total_min,
                vessels: v.vessels,
                is_default: v.is_default,
            })
            .collect(),
        updated_at: recipe.updated_at,
    })
}

// Écriture

fn check_slug(slug: &str) -> ApiResult<()> {
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

/// Enregistre la recette (création ou remplacement complet). Renvoie `true` si elle a été créée.
async fn save(
    conn: &mut PgConnection,
    household_id: Uuid,
    slug: &str,
    input: &RecipeInput,
) -> ApiResult<bool> {
    check_slug(slug)?;
    if input.slug.as_deref().is_some_and(|s| s != slug) {
        return Err(ApiError::invalid(
            "Le slug du corps diffère de celui de l'adresse.",
        ));
    }
    check(input)?;

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
    let (recipe_id, created): (Uuid, bool) = sqlx::query_as(
        "INSERT INTO recipes (id, household_id, slug, title, search_key, description, tags, season_months, source)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         ON CONFLICT (household_id, slug) DO UPDATE SET
            title = excluded.title, search_key = excluded.search_key, description = excluded.description,
            tags = excluded.tags, season_months = excluded.season_months, source = excluded.source,
            updated_at = now()
         RETURNING id, (xmax = 0)",
    )
    .bind(Uuid::now_v7())
    .bind(household_id)
    .bind(slug)
    .bind(&title)
    .bind(search_key(&title))
    .bind(input.description.trim())
    .bind(&tags)
    .bind(&input.season_months)
    .bind(optional_text(input.source.as_ref()))
    .fetch_one(&mut *conn)
    .await?;

    sqlx::query("DELETE FROM recipe_ingredients WHERE recipe_id = $1")
        .bind(recipe_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM recipe_variants WHERE recipe_id = $1")
        .bind(recipe_id)
        .execute(&mut *conn)
        .await?;

    for (position, line) in input.ingredients.iter().enumerate() {
        let (ingredient_id, default_state) = &known[&line.ingredient];
        let (unit, factor) = line.unit.normalize();
        let state = line
            .state
            .map(|s| s.as_str().to_owned())
            .unwrap_or_else(|| default_state.clone());
        sqlx::query(
            "INSERT INTO recipe_ingredients
                (recipe_id, position, ingredient_id, state, qty, unit, role, optional, per_batch, note)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
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
        sqlx::query(
            "INSERT INTO recipe_variants
                (id, recipe_id, position, label, equipment_codes, active_min, total_min, vessels, is_default)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(variant_id)
        .bind(recipe_id)
        .bind(position as i16)
        .bind(variant.label.trim())
        .bind(&variant.equipment)
        .bind(variant.active_min)
        .bind(variant.total_min)
        .bind(variant.vessels.unwrap_or(1))
        .bind(position == default)
        .execute(&mut *conn)
        .await?;
        for (position, phase) in variant.phases.iter().enumerate() {
            let phase_id = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO variant_phases (id, variant_id, position, title, text, spoken_text, timer_s, day_offset)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(phase_id)
            .bind(variant_id)
            .bind(position as i16)
            .bind(phase.title.trim())
            .bind(phase.text.trim())
            .bind(optional_text(phase.spoken_text.as_ref()))
            .bind(phase.timer_s)
            .bind(phase.day_offset)
            .execute(&mut *conn)
            .await?;
            for (position, step) in phase.steps.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO variant_steps
                        (id, phase_id, position, text, spoken_text, timer_s, video_url, equipment_code, params)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
                )
                .bind(Uuid::now_v7())
                .bind(phase_id)
                .bind(position as i16)
                .bind(step.text.trim())
                .bind(optional_text(step.spoken_text.as_ref()))
                .bind(step.timer_s)
                .bind(&step.video_url)
                .bind(&step.equipment)
                .bind(step.params.clone().unwrap_or_else(|| Value::Object(Default::default())))
                .execute(&mut *conn)
                .await?;
            }
        }
    }
    Ok(created)
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
    description: String,
    tags: Vec<String>,
    season_months: Vec<i16>,
    variant_count: i64,
    #[sqlx(flatten)]
    default_variant: VariantSummary,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RecipeQuery {
    /// Mots cherchés dans le titre et les ingrédients, sans tenir compte des accents ni de la casse.
    pub q: Option<String>,
    /// Étiquette exacte.
    pub tag: Option<String>,
}

/// Recettes du foyer, par titre.
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
        "SELECT r.slug, r.title, r.description, r.tags, r.season_months,
                (SELECT count(*) FROM recipe_variants c WHERE c.recipe_id = r.id) AS variant_count,
                d.label, d.equipment_codes AS equipment, d.active_min, d.total_min, d.vessels
         FROM recipes r JOIN recipe_variants d ON d.recipe_id = r.id AND d.is_default
         WHERE r.household_id = $1
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
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| RecipeSummary {
                slug: r.slug,
                title: r.title,
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

/// Publie une recette : la crée (201) ou la remplace entièrement (200). Idempotent.
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
    let mut tx = state.db.begin().await?;
    let created = save(&mut tx, id, &slug, &body).await?;
    tx.commit().await?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(load(&state.db, id, &slug).await?)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/households/{id}/recipes/{slug}",
    tag = "recettes",
    params(("id" = Uuid, Path), ("slug" = String, Path)),
    responses(
        (status = 204),
        (status = 403, body = ErrorBody),
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
    let in_menu: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM menu_slots s JOIN recipes r ON r.id = s.recipe_id
                        WHERE r.household_id = $1 AND r.slug = $2)",
    )
    .bind(id)
    .bind(&slug)
    .fetch_one(&state.db)
    .await?;
    if in_menu {
        return Err(ApiError::conflict(
            "recipe_in_menu",
            "Cette recette est prévue dans un menu : retirez-la du menu d'abord.",
        ));
    }
    let deleted = sqlx::query("DELETE FROM recipes WHERE household_id = $1 AND slug = $2")
        .bind(id)
        .bind(&slug)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
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

/// Ce que l'import a fait.
#[derive(Serialize, ToSchema)]
pub struct ImportResult {
    /// Codes des ingrédients créés.
    pub ingredients_created: Vec<String>,
    pub recipes_created: Vec<String>,
    pub recipes_updated: Vec<String>,
    /// Le lot avait un menu, et il est publié.
    pub menu_imported: bool,
    /// Avertissements du menu publié : conflits d'appareil, matériel absent, exclusions…
    pub warnings: Vec<Warning>,
}

/// Importe un lot en une transaction : tout ou rien. Idempotent (clé : nom des ingrédients, `slug` des recettes,
/// `week_start` du menu). Recettes et ingrédients demandent `recipes:write`, le menu `menus:write`.
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
    let mut result = ImportResult {
        ingredients_created: Vec::new(),
        recipes_created: Vec::new(),
        recipes_updated: Vec::new(),
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
        }
    }
    for recipe in &lot.recipes {
        let Some(slug) = recipe.slug.as_deref() else {
            return Err(ApiError::invalid(format!(
                "{} : chaque recette du lot a un slug.",
                recipe.title
            )));
        };
        let created = save(&mut tx, id, slug, recipe)
            .await
            .map_err(|e| e.context(slug))?;
        if created {
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
        result.warnings = menus::read(&state.db, id, week_start).await?.warnings;
    }
    Ok(Json(result))
}
