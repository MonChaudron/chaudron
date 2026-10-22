//! Liste de courses d'un menu : calculée à la lecture depuis les créneaux (quantités du créneau, sommées par
//! ingrédient, forme d'achat et unité), rangée par rayon. Les cases cochées et les ajouts à la main sont des
//! opérations horodatées par le téléphone : idempotentes (`op_id`), la dernière écriture gagne, article par article.

use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection, PgPool};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    AppState,
    auth::Caller,
    error::{ApiError, ApiResult, ErrorBody},
    menus::{self, Day},
    substitutions::{Catalog, Offer},
    text::search_key,
    types::{Aisle, IngredientState, PortionRole, Scope, Unit},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/households/{id}/menus/{week_start}/shopping",
            get(read_list),
        )
        .route(
            "/households/{id}/shopping/ops",
            get(list_ops).post(apply_ops),
        )
}

crate::types::text_enum! {
    /// Bloc de la liste.
    pub enum Block {
        /// Calculé depuis les recettes du menu.
        Recipes = "recipes",
        /// Petits-déjeuners et desserts, ajoutés à la main.
        BreakfastDesserts = "breakfast_desserts",
        /// Le reste : une fête, un oubli.
        Special = "special",
    }
}

crate::types::text_enum! {
    /// Opération sur un article.
    pub enum OpKind {
        Check = "check",
        Uncheck = "uncheck",
        /// Ajout à la main (`key` = `manual:{uuid}`, choisi par le téléphone).
        Add = "add",
        /// Modifier un ajout à la main.
        Edit = "edit",
        /// Retirer un ajout à la main.
        Remove = "remove",
        /// Choisir un remplacement pour un article calculé (`substitution_id`), ou revenir à l'ingrédient (nul).
        Substitute = "substitute",
    }
}

/// Article de la liste.
#[derive(Serialize, ToSchema)]
pub struct ShoppingItem {
    /// Clé stable de l'article : `{ingredient_id}:{état}:{unité}`, ou `manual:{uuid}`.
    pub key: String,
    pub label: String,
    /// Code de l'ingrédient (article calculé).
    pub ingredient: Option<String>,
    pub state: Option<IngredientState>,
    /// Nulle : « au goût ». Les pièces sont arrondies à l'unité supérieure.
    pub qty: Option<f32>,
    pub unit: Option<Unit>,
    pub aisle: Aisle,
    pub block: Block,
    /// Cuillères, pincées, « au goût » : au placard, à vérifier plutôt qu'à acheter.
    pub pantry: bool,
    /// Facultatif dans toutes les recettes qui l'utilisent.
    pub optional: bool,
    pub manual: bool,
    /// Recettes du menu qui l'utilisent.
    pub recipes: Vec<String>,
    pub checked: bool,
    /// Nom de qui a coché.
    pub checked_by: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub checked_at: Option<OffsetDateTime>,
    /// Hausse de la quantité depuis que la case a été cochée (le menu a changé).
    pub grown: Option<f32>,
    /// Les plats pour lesquels cet ingrédient frais ne tiendra pas : à congeler en rentrant (ou déjà congelé).
    pub freeze: Vec<FreezeHint>,
    /// Les remplacements possibles (« lieu noir, 1 kg ») ; aucun qu'une recette de la ligne refuse.
    pub substitutes: Vec<Offer>,
    /// Le remplacement choisi : l'article dit alors le remplaçant, sa quantité et son rayon.
    pub substitution: Option<Uuid>,
    /// Ce qu'il remplace.
    pub replaces: Option<Replaced>,
}

/// L'ingrédient d'une ligne remplacée.
#[derive(Serialize, ToSchema)]
pub struct Replaced {
    pub ingredient: String,
    pub label: String,
    pub qty: Option<f32>,
}

/// Un plat dont la part de cet article est à congeler.
#[derive(Serialize, ToSchema)]
pub struct FreezeHint {
    #[schema(value_type = String, format = Date)]
    pub date: Day,
    pub recipe: String,
    pub frozen: bool,
}

/// Liste de courses d'un menu.
#[derive(Serialize, ToSchema)]
pub struct ShoppingList {
    #[schema(value_type = String, format = Date)]
    pub week_start: Day,
    /// Dernier numéro du journal des opérations du foyer.
    pub cursor: i64,
    /// Bloc des recettes (rayons dans l'ordre du magasin, placard à la fin), puis petits-déjeuners et desserts,
    /// puis le reste.
    pub items: Vec<ShoppingItem>,
}

/// Opération d'un téléphone, rejouée telle quelle s'il était hors ligne.
#[derive(Deserialize, ToSchema)]
pub struct ShoppingOp {
    /// Choisi par le téléphone : rejouer la même opération ne fait rien.
    pub op_id: Uuid,
    /// Heure du geste sur le téléphone (bornée à l'heure du serveur).
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub at: OffsetDateTime,
    #[schema(value_type = String, format = Date)]
    pub week_start: Day,
    pub kind: OpKind,
    pub key: String,
    pub label: Option<String>,
    pub qty: Option<f32>,
    pub unit: Option<Unit>,
    pub aisle: Option<Aisle>,
    pub block: Option<Block>,
    /// `substitute` : le remplacement choisi ; nul, retour à l'ingrédient.
    pub substitution_id: Option<Uuid>,
}

#[derive(Deserialize, ToSchema)]
pub struct OpsInput {
    pub ops: Vec<ShoppingOp>,
}

#[derive(Serialize, ToSchema)]
pub struct OpsResult {
    /// Opérations appliquées (ni déjà vues, ni plus anciennes que l'état de l'article).
    pub applied: usize,
    pub cursor: i64,
}

/// Opération du journal.
#[derive(Serialize, ToSchema, FromRow)]
pub struct JournalOp {
    pub seq: i64,
    pub op_id: Uuid,
    #[sqlx(try_from = "time::Date")]
    #[schema(value_type = String, format = Date)]
    pub week_start: DayColumn,
    #[sqlx(try_from = "String")]
    pub kind: OpKind,
    pub key: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub at: OffsetDateTime,
    /// Nom de qui l'a faite.
    pub by: Option<String>,
}

/// Une date lue en base, écrite `2026-10-18`.
#[derive(Serialize)]
#[serde(transparent)]
pub struct DayColumn(Day);

impl From<time::Date> for DayColumn {
    fn from(date: time::Date) -> Self {
        Self(Day(date))
    }
}

#[derive(Serialize, ToSchema)]
pub struct Journal {
    pub cursor: i64,
    pub ops: Vec<JournalOp>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct JournalQuery {
    /// Dernier numéro déjà vu (0 : tout).
    #[serde(default)]
    pub since: i64,
}

// Calcul

/// Article calculé, avant fusion avec l'état stocké.
struct Computed {
    key: String,
    ingredient_id: Uuid,
    /// Recettes du menu qui la composent (identifiants).
    recipe_ids: Vec<Uuid>,
    label: String,
    ingredient: String,
    state: IngredientState,
    qty: Option<f32>,
    unit: Unit,
    aisle: Aisle,
    pantry: bool,
    optional: bool,
    recipes: Vec<String>,
}

#[derive(FromRow)]
struct LineRow {
    recipe_id: Uuid,
    slug: String,
    title: String,
    ingredient_id: Uuid,
    code: String,
    name: String,
    #[sqlx(try_from = "String")]
    aisle: Aisle,
    #[sqlx(try_from = "String")]
    state: IngredientState,
    qty: Option<f32>,
    #[sqlx(try_from = "String")]
    unit: Unit,
    #[sqlx(try_from = "String")]
    role: PortionRole,
    optional: bool,
    per_batch: bool,
}

fn aisle_order(aisle: Aisle) -> u8 {
    match aisle {
        Aisle::Produce => 0,
        Aisle::Fresh => 1,
        Aisle::Butcher => 2,
        Aisle::Fish => 3,
        Aisle::Dairy => 4,
        Aisle::Bakery => 5,
        Aisle::Grocery => 6,
        Aisle::Other => 7,
        // Les surgelés en dernier : la chaîne du froid.
        Aisle::Frozen => 8,
    }
}

fn block_order(block: Block) -> u8 {
    match block {
        Block::Recipes => 0,
        Block::BreakfastDesserts => 1,
        Block::Special => 2,
    }
}

/// Le rayon dépend de la forme d'achat : un épinard surgelé n'est pas aux fruits et légumes.
fn aisle_for(state: IngredientState, aisle: Aisle) -> Aisle {
    match state {
        IngredientState::Frozen => Aisle::Frozen,
        IngredientState::Canned | IngredientState::Dried => Aisle::Grocery,
        IngredientState::Fresh | IngredientState::Cooked => aisle,
    }
}

/// Les articles calculés d'un menu, par clé.
async fn compute(
    db: &PgPool,
    household_id: Uuid,
    week_start: Day,
) -> ApiResult<(Vec<Computed>, menus::Menu)> {
    let menu = menus::read(db, household_id, week_start).await?;
    let mut slugs: Vec<&str> = menu.slots.iter().map(|s| s.recipe.slug.as_str()).collect();
    slugs.sort_unstable();
    slugs.dedup();
    let lines: Vec<LineRow> = sqlx::query_as(
        "SELECT r.id AS recipe_id, r.slug, r.title, l.ingredient_id, i.code, i.name, i.aisle, l.state, l.qty, l.unit, l.role,
                l.optional, l.per_batch
         FROM recipe_ingredients l JOIN recipes r ON r.id = l.recipe_id JOIN ingredients i ON i.id = l.ingredient_id
         WHERE (r.household_id = $1 OR r.household_id IS NULL) AND r.slug = ANY($2) ORDER BY l.position",
    )
    .bind(household_id)
    .bind(&slugs)
    .fetch_all(db)
    .await?;
    let mut by_recipe: HashMap<&str, Vec<&LineRow>> = HashMap::new();
    for line in &lines {
        by_recipe.entry(line.slug.as_str()).or_default().push(line);
    }

    let mut items: Vec<Computed> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for slot in &menu.slots {
        for line in by_recipe
            .get(slot.recipe.slug.as_str())
            .into_iter()
            .flatten()
        {
            let key = format!(
                "{}:{}:{}",
                line.ingredient_id,
                line.state.as_str(),
                line.unit.as_str()
            );
            let qty = line.qty.map(|q| {
                if line.per_batch {
                    q
                } else {
                    q * slot.scale.for_role(line.role)
                }
            });
            let i = *index.entry(key.clone()).or_insert_with(|| {
                items.push(Computed {
                    key,
                    ingredient_id: line.ingredient_id,
                    recipe_ids: Vec::new(),
                    label: line.name.clone(),
                    ingredient: line.code.clone(),
                    state: line.state,
                    qty: None,
                    unit: line.unit,
                    aisle: aisle_for(line.state, line.aisle),
                    pantry: true,
                    optional: true,
                    recipes: Vec::new(),
                });
                items.len() - 1
            });
            let item = &mut items[i];
            if let Some(q) = qty {
                item.qty = Some(item.qty.unwrap_or(0.0) + q);
            }
            item.optional &= line.optional;
            if !item.recipes.contains(&line.title) {
                item.recipes.push(line.title.clone());
            }
            if !item.recipe_ids.contains(&line.recipe_id) {
                item.recipe_ids.push(line.recipe_id);
            }
        }
    }
    for item in &mut items {
        if item.unit == Unit::Piece {
            item.qty = item.qty.map(|q| (q - 1e-4).ceil().max(1.0));
        }
        item.pantry =
            item.qty.is_none() || matches!(item.unit, Unit::Tbsp | Unit::Tsp | Unit::Pinch);
    }
    Ok((items, menu))
}

/// À congeler : les alertes de fraîcheur du menu sur cet ingrédient, et ce qui est déjà congelé pour un plat.
fn freeze_hints(menu: &menus::Menu, code: &str) -> Vec<FreezeHint> {
    let title = |slot: Uuid| {
        menu.slots
            .iter()
            .find(|s| s.id == slot)
            .map(|s| s.recipe.title.clone())
            .unwrap_or_default()
    };
    let mut hints: Vec<FreezeHint> = menu
        .warnings
        .iter()
        .filter(|w| {
            w.kind == menus::WarningKind::Freshness && w.ingredient.as_deref() == Some(code)
        })
        .map(|w| FreezeHint {
            date: w.date,
            recipe: w.slots.first().map(|s| title(*s)).unwrap_or_default(),
            frozen: false,
        })
        .chain(
            menu.slots
                .iter()
                .filter(|s| s.thaw.iter().any(|t| t.ingredient == code))
                .map(|s| FreezeHint {
                    date: s.date,
                    recipe: s.recipe.title.clone(),
                    frozen: true,
                }),
        )
        .collect();
    hints.sort_by_key(|h| h.date);
    hints
}

#[derive(FromRow)]
struct StateRow {
    key: String,
    substitution_id: Option<Uuid>,
    manual: bool,
    label: Option<String>,
    qty: Option<f32>,
    #[sqlx(try_from = "Option<String>")]
    unit: OptionalUnit,
    #[sqlx(try_from = "Option<String>")]
    aisle: OptionalAisle,
    #[sqlx(try_from = "String")]
    block: Block,
    checked: bool,
    checked_by: Option<String>,
    checked_at: Option<OffsetDateTime>,
    qty_at_check: Option<f32>,
    removed: bool,
}

struct OptionalUnit(Option<Unit>);
struct OptionalAisle(Option<Aisle>);

impl TryFrom<Option<String>> for OptionalUnit {
    type Error = String;
    fn try_from(value: Option<String>) -> Result<Self, String> {
        value.map(Unit::try_from).transpose().map(Self)
    }
}

impl TryFrom<Option<String>> for OptionalAisle {
    type Error = String;
    fn try_from(value: Option<String>) -> Result<Self, String> {
        value.map(Aisle::try_from).transpose().map(Self)
    }
}

async fn menu_id(db: &PgPool, household_id: Uuid, week_start: Day) -> ApiResult<Uuid> {
    sqlx::query_scalar("SELECT id FROM menus WHERE household_id = $1 AND week_start = $2")
        .bind(household_id)
        .bind(week_start.0)
        .fetch_optional(db)
        .await?
        .ok_or_else(ApiError::not_found)
}

async fn cursor(db: &PgPool, household_id: Uuid) -> ApiResult<i64> {
    Ok(
        sqlx::query_scalar(
            "SELECT coalesce(max(seq), 0) FROM shopping_ops WHERE household_id = $1",
        )
        .bind(household_id)
        .fetch_one(db)
        .await?,
    )
}

pub async fn list(db: &PgPool, household_id: Uuid, week_start: Day) -> ApiResult<ShoppingList> {
    let menu_id = menu_id(db, household_id, week_start).await?;
    let (computed, menu) = compute(db, household_id, week_start).await?;
    let states: Vec<StateRow> = sqlx::query_as(
        "SELECT s.key, s.substitution_id, s.manual, s.label, s.qty, s.unit, s.aisle, s.block, s.checked, u.display_name AS checked_by,
                s.checked_at, s.qty_at_check, s.removed
         FROM shopping_items s LEFT JOIN users u ON u.id = s.checked_by WHERE s.menu_id = $1",
    )
    .bind(menu_id)
    .fetch_all(db)
    .await?;
    let by_key: HashMap<&str, &StateRow> = states.iter().map(|s| (s.key.as_str(), s)).collect();
    let catalog = catalog(db, &computed).await?;

    let mut items: Vec<ShoppingItem> = computed
        .into_iter()
        .map(|c| {
            let state = by_key.get(c.key.as_str());
            let substitutes = catalog.offers(c.ingredient_id, &c.recipe_ids, c.qty);
            // Le remplacement choisi, s'il vaut encore (refusé depuis, ou supprimé : oublié sans bruit).
            let chosen = state
                .and_then(|s| s.substitution_id)
                .and_then(|id| substitutes.iter().find(|o| o.id == id))
                .cloned();
            let checked = state.is_some_and(|s| s.checked);
            let grown = match (checked, state.and_then(|s| s.qty_at_check), c.qty) {
                (true, Some(before), Some(now)) if now > before + 1e-3 => Some(now - before),
                _ => None,
            };
            let freeze = freeze_hints(&menu, &c.ingredient);
            let replaces = chosen.as_ref().map(|_| Replaced {
                ingredient: c.ingredient.clone(),
                label: c.label.clone(),
                qty: c.qty,
            });
            ShoppingItem {
                key: c.key,
                label: chosen.as_ref().map_or(c.label, |o| o.label.clone()),
                ingredient: Some(
                    chosen
                        .as_ref()
                        .map_or(c.ingredient, |o| o.ingredient.clone()),
                ),
                state: Some(c.state),
                qty: chosen.as_ref().map_or(c.qty, |o| o.qty),
                unit: Some(c.unit),
                aisle: chosen
                    .as_ref()
                    .and_then(|o| Aisle::try_from(o.aisle.clone()).ok())
                    .map_or(c.aisle, |a| aisle_for(c.state, a)),
                block: Block::Recipes,
                pantry: c.pantry,
                optional: c.optional,
                manual: false,
                recipes: c.recipes,
                checked,
                checked_by: state.filter(|_| checked).and_then(|s| s.checked_by.clone()),
                checked_at: state.filter(|_| checked).and_then(|s| s.checked_at),
                grown,
                freeze,
                substitution: chosen.as_ref().map(|o| o.id),
                substitutes,
                replaces,
            }
        })
        .collect();
    for s in states.iter().filter(|s| s.manual && !s.removed) {
        items.push(ShoppingItem {
            key: s.key.clone(),
            label: s.label.clone().unwrap_or_default(),
            ingredient: None,
            state: None,
            qty: s.qty,
            unit: s.unit.0,
            aisle: s.aisle.0.unwrap_or(Aisle::Other),
            block: s.block,
            pantry: false,
            optional: false,
            manual: true,
            recipes: Vec::new(),
            checked: s.checked,
            checked_by: s.checked_by.clone().filter(|_| s.checked),
            checked_at: s.checked_at.filter(|_| s.checked),
            grown: None,
            freeze: Vec::new(),
            substitutes: Vec::new(),
            substitution: None,
            replaces: None,
        });
    }
    items.sort_by_cached_key(|i| {
        (
            block_order(i.block),
            i.pantry,
            // Le placard est une seule section, par ordre alphabétique.
            if i.pantry { 0 } else { aisle_order(i.aisle) },
            search_key(&i.label),
        )
    });
    Ok(ShoppingList {
        week_start,
        cursor: cursor(db, household_id).await?,
        items,
    })
}

/// Les remplacements des ingrédients de la liste, et ce qu'en disent ses recettes.
async fn catalog(db: &PgPool, computed: &[Computed]) -> ApiResult<Catalog> {
    let ingredients: Vec<Uuid> = computed.iter().map(|c| c.ingredient_id).collect();
    let mut recipes: Vec<Uuid> = computed
        .iter()
        .flat_map(|c| c.recipe_ids.iter().copied())
        .collect();
    recipes.sort_unstable();
    recipes.dedup();
    Catalog::load(&mut *db.acquire().await?, &ingredients, &recipes).await
}

// Opérations

fn label(value: Option<&String>) -> ApiResult<Option<String>> {
    match value.map(|v| v.trim()) {
        None => Ok(None),
        Some(v) if !v.is_empty() && v.chars().count() <= 120 => Ok(Some(v.to_owned())),
        Some(_) => Err(ApiError::invalid(
            "Un article fait entre 1 et 120 caractères.",
        )),
    }
}

/// Applique une opération dans la transaction ; `false` si elle était déjà vue ou plus ancienne que l'article.
async fn apply(
    conn: &mut PgConnection,
    household_id: Uuid,
    menu_id: Uuid,
    user: Option<Uuid>,
    op: &ShoppingOp,
    at: OffsetDateTime,
    computed: &HashMap<String, (Option<f32>, Vec<Uuid>)>,
) -> ApiResult<bool> {
    let manual: Option<(bool, OffsetDateTime)> = sqlx::query_as(
        "SELECT removed, op_at FROM shopping_items WHERE menu_id = $1 AND key = $2 AND manual",
    )
    .bind(menu_id)
    .bind(&op.key)
    .fetch_optional(&mut *conn)
    .await?;
    let is_computed = computed.contains_key(&op.key);
    match op.kind {
        OpKind::Add => {
            if !op.key.starts_with("manual:") || op.key.len() > 80 {
                return Err(ApiError::invalid("Un ajout a une clé « manual:… »."));
            }
            if label(op.label.as_ref())?.is_none() {
                return Err(ApiError::invalid("Un ajout a un libellé."));
            }
        }
        OpKind::Substitute if !is_computed => {
            return Err(ApiError::invalid(format!(
                "{} : on remplace un article des recettes.",
                op.key
            )));
        }
        OpKind::Substitute
            if op
                .substitution_id
                .is_some_and(|id| !computed[&op.key].1.contains(&id)) =>
        {
            return Err(ApiError::invalid(
                "Ce remplacement ne vaut pas pour cet article (une recette le refuse ?).",
            ));
        }
        OpKind::Edit | OpKind::Remove if manual.is_none() => {
            return Err(ApiError::invalid(format!(
                "{} : seul un ajout à la main se modifie ou se retire.",
                op.key
            )));
        }
        OpKind::Check | OpKind::Uncheck
            if !is_computed && manual.is_none_or(|(removed, _)| removed) =>
        {
            return Err(ApiError::invalid(format!("{} : article inconnu.", op.key)));
        }
        _ => {}
    }
    if op.qty.is_some_and(|q| !(q > 0.0 && q.is_finite())) {
        return Err(ApiError::invalid("La quantité doit être positive."));
    }

    // Idempotence : une opération déjà vue ne fait rien.
    let logged = sqlx::query(
        "INSERT INTO shopping_ops (household_id, menu_id, op_id, at, user_id, kind, key)
         VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (op_id) DO NOTHING",
    )
    .bind(household_id)
    .bind(menu_id)
    .bind(op.op_id)
    .bind(at)
    .bind(user)
    .bind(op.kind.as_str())
    .bind(&op.key)
    .execute(&mut *conn)
    .await?;
    if logged.rows_affected() == 0 {
        return Ok(false);
    }

    let applied = match op.kind {
        OpKind::Check | OpKind::Uncheck => {
            let checked = op.kind == OpKind::Check;
            let qty_now = computed.get(&op.key).and_then(|c| c.0);
            sqlx::query(
                "INSERT INTO shopping_items (id, menu_id, key, manual, checked, checked_by, checked_at, qty_at_check, op_at)
                 VALUES ($1, $2, $3, false, $4, $5, $6, $7, $8)
                 ON CONFLICT (menu_id, key) DO UPDATE SET
                    checked = excluded.checked, checked_by = excluded.checked_by, checked_at = excluded.checked_at,
                    qty_at_check = CASE WHEN shopping_items.manual THEN NULL ELSE excluded.qty_at_check END,
                    op_at = excluded.op_at
                 WHERE shopping_items.op_at <= excluded.op_at",
            )
            .bind(Uuid::now_v7())
            .bind(menu_id)
            .bind(&op.key)
            .bind(checked)
            .bind(user.filter(|_| checked))
            .bind(checked.then_some(at))
            .bind(qty_now.filter(|_| checked))
            .bind(at)
            .execute(&mut *conn)
            .await?
        }
        OpKind::Add | OpKind::Edit => {
            sqlx::query(
                "INSERT INTO shopping_items (id, menu_id, key, manual, label, qty, unit, aisle, block, op_at)
                 VALUES ($1, $2, $3, true, $4, $5, $6, $7, coalesce($8, 'special'), $9)
                 ON CONFLICT (menu_id, key) DO UPDATE SET
                    label = coalesce(excluded.label, shopping_items.label),
                    qty = coalesce(excluded.qty, shopping_items.qty),
                    unit = coalesce(excluded.unit, shopping_items.unit),
                    aisle = coalesce(excluded.aisle, shopping_items.aisle),
                    block = CASE WHEN $8 IS NULL THEN shopping_items.block ELSE excluded.block END,
                    removed = false, op_at = excluded.op_at
                 WHERE shopping_items.manual AND shopping_items.op_at <= excluded.op_at",
            )
            .bind(Uuid::now_v7())
            .bind(menu_id)
            .bind(&op.key)
            .bind(label(op.label.as_ref())?)
            .bind(op.qty)
            .bind(op.unit.map(Unit::as_str))
            .bind(op.aisle.map(Aisle::as_str))
            .bind(op.block.map(Block::as_str))
            .bind(at)
            .execute(&mut *conn)
            .await?
        }
        OpKind::Substitute => {
            sqlx::query(
                "INSERT INTO shopping_items (id, menu_id, key, manual, substitution_id, op_at)
                 VALUES ($1, $2, $3, false, $4, $5)
                 ON CONFLICT (menu_id, key) DO UPDATE SET
                    substitution_id = excluded.substitution_id, op_at = excluded.op_at
                 WHERE NOT shopping_items.manual AND shopping_items.op_at <= excluded.op_at",
            )
            .bind(Uuid::now_v7())
            .bind(menu_id)
            .bind(&op.key)
            .bind(op.substitution_id)
            .bind(at)
            .execute(&mut *conn)
            .await?
        }
        OpKind::Remove => {
            sqlx::query(
                "UPDATE shopping_items SET removed = true, op_at = $3
                 WHERE menu_id = $1 AND key = $2 AND manual AND op_at <= $3",
            )
            .bind(menu_id)
            .bind(&op.key)
            .bind(at)
            .execute(&mut *conn)
            .await?
        }
    };
    if applied.rows_affected() == 0 {
        // Plus ancienne que l'article : elle ne change rien, le journal ne la garde pas.
        sqlx::query("DELETE FROM shopping_ops WHERE op_id = $1")
            .bind(op.op_id)
            .execute(&mut *conn)
            .await?;
        return Ok(false);
    }
    Ok(true)
}

// Routes

/// Liste de courses d'un menu, calculée depuis ses créneaux, avec les cases cochées et les ajouts à la main.
#[utoipa::path(
    get,
    path = "/api/v1/households/{id}/menus/{week_start}/shopping",
    tag = "courses",
    params(("id" = Uuid, Path), ("week_start" = String, Path, format = Date)),
    responses(
        (status = 200, body = ShoppingList),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn read_list(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, week_start)): Path<(Uuid, Day)>,
) -> ApiResult<Json<ShoppingList>> {
    caller.require(&state.db, id, Scope::Read).await?;
    list(&state.db, id, week_start).await.map(Json)
}

/// Applique des opérations sur les listes du foyer (cocher, décocher, ajouter, modifier, retirer), dans l'ordre,
/// tout ou rien. Rejouables : une opération déjà vue ou plus ancienne que l'article ne change rien.
#[utoipa::path(
    post,
    path = "/api/v1/households/{id}/shopping/ops",
    tag = "courses",
    params(("id" = Uuid, Path)),
    request_body = OpsInput,
    responses(
        (status = 200, body = OpsResult),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn apply_ops(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Json(body): Json<OpsInput>,
) -> ApiResult<Json<OpsResult>> {
    caller.require(&state.db, id, Scope::MenusWrite).await?;
    let user = match &caller {
        Caller::User(user) => Some(*user),
        Caller::Token(_) => None,
    };
    if body.ops.len() > 500 {
        return Err(ApiError::invalid("500 opérations au plus par envoi."));
    }
    // Les menus concernés et leurs articles calculés (pour valider les clés et noter la quantité cochée).
    type Computed = HashMap<String, (Option<f32>, Vec<Uuid>)>;
    let mut menus: HashMap<Day, (Uuid, Computed)> = HashMap::new();
    for op in &body.ops {
        if let std::collections::hash_map::Entry::Vacant(entry) = menus.entry(op.week_start) {
            let menu_id = menu_id(&state.db, id, op.week_start)
                .await
                .map_err(|_| ApiError::invalid(format!("Pas de menu le {}.", op.week_start)))?;
            let items = compute(&state.db, id, op.week_start).await?.0;
            let catalog = catalog(&state.db, &items).await?;
            let computed = items
                .into_iter()
                .map(|c| {
                    let offers = catalog
                        .offers(c.ingredient_id, &c.recipe_ids, c.qty)
                        .into_iter()
                        .map(|o| o.id)
                        .collect();
                    (c.key, (c.qty, offers))
                })
                .collect();
            entry.insert((menu_id, computed));
        }
    }
    let now = OffsetDateTime::now_utc();
    let mut applied = 0;
    let mut tx = state.db.begin().await?;
    for op in &body.ops {
        let (menu_id, computed) = &menus[&op.week_start];
        if apply(&mut tx, id, *menu_id, user, op, op.at.min(now), computed).await? {
            applied += 1;
        }
    }
    tx.commit().await?;
    let cursor = cursor(&state.db, id).await?;
    for week_start in menus.keys() {
        state.notify(id, "shopping", Some(*week_start), Some(cursor));
    }
    Ok(Json(OpsResult { applied, cursor }))
}

/// Journal des opérations du foyer après `since` (500 au plus) : un téléphone qui revient en ligne rattrape ce qu'il
/// a manqué.
#[utoipa::path(
    get,
    path = "/api/v1/households/{id}/shopping/ops",
    tag = "courses",
    params(("id" = Uuid, Path), JournalQuery),
    responses(
        (status = 200, body = Journal),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn list_ops(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Query(query): Query<JournalQuery>,
) -> ApiResult<Json<Journal>> {
    caller.require(&state.db, id, Scope::Read).await?;
    let ops: Vec<JournalOp> = sqlx::query_as(
        "SELECT o.seq, o.op_id, m.week_start, o.kind, o.key, o.at, u.display_name AS by
         FROM shopping_ops o JOIN menus m ON m.id = o.menu_id LEFT JOIN users u ON u.id = o.user_id
         WHERE o.household_id = $1 AND o.seq > $2 ORDER BY o.seq LIMIT 500",
    )
    .bind(id)
    .bind(query.since)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(Journal {
        cursor: cursor(&state.db, id).await?,
        ops,
    }))
}
