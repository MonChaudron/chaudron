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
async fn compute(db: &PgPool, household_id: Uuid, week_start: Day) -> ApiResult<Vec<Computed>> {
    let menu = menus::read(db, household_id, week_start).await?;
    let mut slugs: Vec<&str> = menu.slots.iter().map(|s| s.recipe.slug.as_str()).collect();
    slugs.sort_unstable();
    slugs.dedup();
    let lines: Vec<LineRow> = sqlx::query_as(
        "SELECT r.slug, r.title, l.ingredient_id, i.code, i.name, i.aisle, l.state, l.qty, l.unit, l.role,
                l.optional, l.per_batch
         FROM recipe_ingredients l JOIN recipes r ON r.id = l.recipe_id JOIN ingredients i ON i.id = l.ingredient_id
         WHERE r.household_id = $1 AND r.slug = ANY($2) ORDER BY l.position",
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
                    q * match line.role {
                        PortionRole::Protein => slot.scale.protein,
                        PortionRole::Starch => slot.scale.starch,
                        PortionRole::Veg => slot.scale.veg,
                        PortionRole::Fat => slot.scale.fat,
                        PortionRole::Other => slot.scale.other,
                    }
                }
            });
            let i = *index.entry(key.clone()).or_insert_with(|| {
                items.push(Computed {
                    key,
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
        }
    }
    for item in &mut items {
        if item.unit == Unit::Piece {
            item.qty = item.qty.map(|q| (q - 1e-4).ceil().max(1.0));
        }
        item.pantry =
            item.qty.is_none() || matches!(item.unit, Unit::Tbsp | Unit::Tsp | Unit::Pinch);
    }
    Ok(items)
}

#[derive(FromRow)]
struct StateRow {
    key: String,
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
    let computed = compute(db, household_id, week_start).await?;
    let states: Vec<StateRow> = sqlx::query_as(
        "SELECT s.key, s.manual, s.label, s.qty, s.unit, s.aisle, s.block, s.checked, u.display_name AS checked_by,
                s.checked_at, s.qty_at_check, s.removed
         FROM shopping_items s LEFT JOIN users u ON u.id = s.checked_by WHERE s.menu_id = $1",
    )
    .bind(menu_id)
    .fetch_all(db)
    .await?;
    let by_key: HashMap<&str, &StateRow> = states.iter().map(|s| (s.key.as_str(), s)).collect();

    let mut items: Vec<ShoppingItem> = computed
        .into_iter()
        .map(|c| {
            let state = by_key.get(c.key.as_str());
            let checked = state.is_some_and(|s| s.checked);
            let grown = match (checked, state.and_then(|s| s.qty_at_check), c.qty) {
                (true, Some(before), Some(now)) if now > before + 1e-3 => Some(now - before),
                _ => None,
            };
            ShoppingItem {
                key: c.key,
                label: c.label,
                ingredient: Some(c.ingredient),
                state: Some(c.state),
                qty: c.qty,
                unit: Some(c.unit),
                aisle: c.aisle,
                block: Block::Recipes,
                pantry: c.pantry,
                optional: c.optional,
                manual: false,
                recipes: c.recipes,
                checked,
                checked_by: state.filter(|_| checked).and_then(|s| s.checked_by.clone()),
                checked_at: state.filter(|_| checked).and_then(|s| s.checked_at),
                grown,
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
    computed: &HashMap<String, Option<f32>>,
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
            let qty_now = computed.get(&op.key).copied().flatten();
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
    let mut menus: HashMap<Day, (Uuid, HashMap<String, Option<f32>>)> = HashMap::new();
    for op in &body.ops {
        if let std::collections::hash_map::Entry::Vacant(entry) = menus.entry(op.week_start) {
            let menu_id = menu_id(&state.db, id, op.week_start)
                .await
                .map_err(|_| ApiError::invalid(format!("Pas de menu le {}.", op.week_start)))?;
            let computed = compute(&state.db, id, op.week_start)
                .await?
                .into_iter()
                .map(|c| (c.key, c.qty))
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
