//! Apports d'une portion (design-nutrition.md) : la note d'équilibre d'une recette, les apports de ta portion, le
//! poids cuit à servir. Calculés à partir de la table Ciqual et des quantités, jamais devinés.

pub mod portion;
pub mod score;

use std::collections::HashMap;

use serde::Serialize;
use sqlx::{FromRow, PgPool};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    error::ApiResult,
    members::{self, Portions},
    menus::Menu,
    types::{Grade, IngredientState, PortionRole, Unit, text_enum},
};

use portion::{Food, Line, compute};

text_enum! {
    /// La portion décrite : celle du membre relié au compte, ou une portion standard.
    pub enum PortionKind {
        Member = "member",
        Standard = "standard",
    }
}

text_enum! {
    /// Un critère de la note.
    pub enum Criterion {
        Energy = "energy",
        Sugars = "sugars",
        SatFat = "sat_fat",
        Salt = "salt",
        Protein = "protein",
        Fiber = "fiber",
        /// Fruits, légumes et légumineuses.
        Fvl = "fvl",
    }
}

/// Apports d'une portion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, ToSchema)]
pub struct Amounts {
    pub energy_kcal: f32,
    pub protein_g: f32,
    pub carbs_g: f32,
    pub sugars_g: f32,
    pub fat_g: f32,
    pub sat_fat_g: f32,
    pub fiber_g: f32,
    pub salt_g: f32,
}

/// Part des apports de référence d'un adulte (%), arrondie.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, ToSchema)]
pub struct DailyPct {
    pub energy_kcal: i32,
    pub protein_g: i32,
    pub carbs_g: i32,
    pub sugars_g: i32,
    pub fat_g: i32,
    pub sat_fat_g: i32,
    pub fiber_g: i32,
    pub salt_g: i32,
}

/// D'où viennent les calories (% de l'énergie) : somme 100, ou tous nuls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, ToSchema)]
pub struct EnergySplit {
    pub protein: i32,
    pub carbs: i32,
    pub fat: i32,
    pub fiber: i32,
}

/// Poids cuit à servir d'un rôle (« Paleron de bœuf », « légumes »).
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Serve {
    pub role: PortionRole,
    pub label: String,
    pub grams: i32,
}

/// Ce qui baisse ou tient la note : un critère, ses points, l'ingrédient qui y contribue le plus.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Factor {
    pub criterion: Criterion,
    pub points: i32,
    pub ingredient: Option<String>,
    /// Ce qu'apporte cet ingrédient à ta portion : kcal pour l'énergie, grammes sinon (crus pour les fruits et
    /// légumes).
    pub amount: Option<f32>,
}

/// Les apports d'une recette pour ta portion, et sa note.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Nutrition {
    /// Note de la recette (portion standard, pour 100 g de plat cuit) ; nulle quand un ingrédient sans données la
    /// fausserait, ou sans rien à peser.
    pub grade: Option<Grade>,
    pub portion: PortionKind,
    pub amounts: Amounts,
    pub daily_pct: DailyPct,
    pub energy_split: EnergySplit,
    /// Poids cuit estimé de ta portion.
    pub cooked_g: i32,
    /// Protéine, féculent, légume, dans cet ordre (les rôles présents).
    pub serve: Vec<Serve>,
    /// Au plus deux, les plus lourds d'abord.
    pub lowers: Vec<Factor>,
    pub holds: Vec<Factor>,
    /// Ingrédients sans données (sans ligne Ciqual, ou dont la quantité ne se pèse pas) : hors des apports.
    pub missing: Vec<String>,
    /// Lignes « au goût », non comptées.
    pub to_taste: Vec<String>,
    /// Aucun ingrédient sans données.
    pub complete: bool,
}

#[derive(FromRow)]
struct LineRow {
    slug: String,
    name: String,
    #[sqlx(try_from = "String")]
    role: PortionRole,
    #[sqlx(try_from = "String")]
    state: IngredientState,
    qty: Option<f32>,
    #[sqlx(try_from = "String")]
    unit: Unit,
    optional: bool,
    per_batch: bool,
    piece_g: Option<f32>,
    density: Option<f32>,
    cooked_ratio: Option<f32>,
    food_code: Option<i32>,
    subgroup_code: Option<String>,
    food_key: Option<String>,
    energy_kcal: Option<f32>,
    protein_g: Option<f32>,
    carbs_g: Option<f32>,
    sugars_g: Option<f32>,
    fat_g: Option<f32>,
    sat_fat_g: Option<f32>,
    fiber_g: Option<f32>,
    salt_g: Option<f32>,
}

impl From<LineRow> for Line {
    fn from(r: LineRow) -> Self {
        let food = r.food_code.map(|_| Food {
            subgroup: r.subgroup_code.unwrap_or_default(),
            name_key: r.food_key.unwrap_or_default(),
            amounts: [
                r.energy_kcal,
                r.protein_g,
                r.carbs_g,
                r.sugars_g,
                r.fat_g,
                r.sat_fat_g,
                r.fiber_g,
                r.salt_g,
            ],
        });
        Line {
            name: r.name,
            role: r.role,
            state: r.state,
            qty: r.qty,
            unit: r.unit,
            optional: r.optional,
            per_batch: r.per_batch,
            piece_g: r.piece_g,
            density: r.density,
            cooked_ratio: r.cooked_ratio,
            food,
        }
    }
}

/// Les lignes des recettes du foyer et de la communauté (une seule avec `slug`), par slug, avec leur ligne Ciqual.
pub(crate) async fn recipe_lines(
    db: &PgPool,
    household_id: Uuid,
    slug: Option<&str>,
) -> ApiResult<HashMap<String, Vec<Line>>> {
    let rows: Vec<LineRow> = sqlx::query_as(
        "SELECT r.slug, i.name, l.role, l.state, l.qty, l.unit, l.optional, l.per_batch,
                i.piece_g, i.density, i.cooked_ratio, f.code AS food_code, f.subgroup_code,
                f.search_key AS food_key, f.energy_kcal, f.protein_g, f.carbs_g, f.sugars_g, f.fat_g,
                f.sat_fat_g, f.fiber_g, f.salt_g
         FROM recipe_ingredients l
         JOIN recipes r ON r.id = l.recipe_id
         JOIN ingredients i ON i.id = l.ingredient_id
         LEFT JOIN ingredient_foods x ON x.ingredient_id = i.id AND x.state = l.state
         LEFT JOIN foods f ON f.code = x.food_code
         WHERE (r.household_id = $1 OR r.household_id IS NULL) AND ($2::text IS NULL OR r.slug = $2)
         ORDER BY r.slug, l.position",
    )
    .bind(household_id)
    .bind(slug)
    .fetch_all(db)
    .await?;
    let mut lines: HashMap<String, Vec<Line>> = HashMap::new();
    for row in rows {
        lines.entry(row.slug.clone()).or_default().push(row.into());
    }
    Ok(lines)
}

/// Le barème du membre relié au compte ; aucun pour un jeton ou un compte sans membre (portion standard).
pub(crate) async fn viewer_portions(
    db: &PgPool,
    household_id: Uuid,
    user: Option<Uuid>,
) -> ApiResult<Option<Portions>> {
    let Some(user) = user else { return Ok(None) };
    Ok(members::load(db, household_id, None)
        .await?
        .into_iter()
        .find(|m| m.user_id == Some(user))
        .map(|m| m.portions))
}

/// Les apports d'une recette pour la portion de qui la lit.
pub(crate) async fn recipe(
    db: &PgPool,
    household_id: Uuid,
    slug: &str,
    user: Option<Uuid>,
) -> ApiResult<Nutrition> {
    let portions = viewer_portions(db, household_id, user).await?;
    let lines = recipe_lines(db, household_id, Some(slug)).await?;
    Ok(compute(
        lines.get(slug).map_or(&[], Vec::as_slice),
        portions.as_ref(),
    ))
}

/// La note de chaque recette, par slug.
pub(crate) async fn grades(
    db: &PgPool,
    household_id: Uuid,
) -> ApiResult<HashMap<String, Option<Grade>>> {
    Ok(recipe_lines(db, household_id, None)
        .await?
        .into_iter()
        .map(|(slug, lines)| (slug, compute(&lines, None).grade))
        .collect())
}

/// Les apports de chaque plat du menu, pour la portion de qui le lit.
pub(crate) async fn nutrition_menu(
    db: &PgPool,
    household_id: Uuid,
    user: Option<Uuid>,
    menu: &mut Menu,
) -> ApiResult<()> {
    if menu.slots.is_empty() {
        return Ok(());
    }
    let portions = viewer_portions(db, household_id, user).await?;
    let lines = recipe_lines(db, household_id, None).await?;
    for slot in &mut menu.slots {
        slot.nutrition = Some(compute(
            lines.get(&slot.recipe.slug).map_or(&[], Vec::as_slice),
            portions.as_ref(),
        ));
    }
    Ok(())
}
