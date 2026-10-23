//! Le prix d'un repas, d'une semaine de courses : quantités × prix unitaires. Un seul calcul, partagé par la liste
//! de courses, le menu et l'estimation à blanc.

use std::collections::HashMap;

use serde::Serialize;
use sqlx::{FromRow, PgPool};
use utoipa::ToSchema;
use uuid::Uuid;

use time::OffsetDateTime;

use crate::{
    AppState,
    error::ApiResult,
    households,
    members::{self, Portions},
    menus::{self, Day, Menu},
    recipes::Lot,
    shopping::{self, LineRow, SlotLines},
    text::{code, search_key},
    types::{Aisle, IngredientState, Meal, PortionRole, Unit, text_enum},
};

use super::{PriceBook, UnitPrice, crossing::line_cents, run};

/// L'estimation chiffre à la volée dix ingrédients nouveaux au plus, en une minute au plus : au-delà, ils restent
/// sans prix (une requête d'un jeton « lire » ne doit pas faire marteler les sources).
const MAX_FRESH: usize = 10;
const FRESH_BUDGET: std::time::Duration = std::time::Duration::from_secs(60);

/// Pour le prix par portion d'une recette, ce qui ne suit pas les portions (un citron, un cube) se partage comme
/// dans un plat de quatre.
const BATCH_PORTIONS: f64 = 4.0;

text_enum! {
    /// Où se place un plat parmi les recettes que le foyer peut choisir : le tiers le moins cher, celui du milieu,
    /// le plus cher (au prix par portion standard).
    pub enum PriceTier {
        Low = "low",
        Mid = "mid",
        High = "high",
    }
}

/// Prix estimé d'un plat du menu, pour ses portions.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct SlotPrice {
    pub cents: i32,
    pub tier: Option<PriceTier>,
    pub complete: bool,
}

/// Prix estimé d'une recette : pour la tablée d'un repas (ou une portion standard), et par portion standard.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct RecipePrice {
    pub cents: i32,
    pub per_portion_cents: i32,
    pub tier: Option<PriceTier>,
    pub complete: bool,
}

/// Coût d'une recette pour une portion standard, par rôle, et ce qui ne suit pas les portions.
#[derive(Clone, Debug, Default)]
pub(crate) struct RoleCost {
    roles: Vec<(PortionRole, f64)>,
    batch: f64,
    complete: bool,
}

impl RoleCost {
    pub(crate) fn for_scale(&self, scale: &Portions) -> f64 {
        self.roles
            .iter()
            .map(|(role, cost)| cost * f64::from(scale.for_role(*role)))
            .sum::<f64>()
            + self.batch
    }

    pub(crate) fn per_portion(&self) -> f64 {
        self.roles.iter().map(|(_, cost)| cost).sum::<f64>() + self.batch / BATCH_PORTIONS
    }

    pub(crate) fn price(&self, scale: Option<&Portions>, tier: Option<PriceTier>) -> RecipePrice {
        let per_portion = self.per_portion();
        RecipePrice {
            cents: scale.map_or(per_portion, |s| self.for_scale(s)).round() as i32,
            per_portion_cents: per_portion.round() as i32,
            tier: tier.filter(|_| self.complete),
            complete: self.complete,
        }
    }
}

#[derive(FromRow)]
struct CostLine {
    slug: String,
    ingredient_id: Uuid,
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

/// Coût de chaque recette que le foyer peut choisir (les siennes, celles de la communauté), par slug.
pub(crate) async fn recipe_costs(
    db: &PgPool,
    household_id: Uuid,
) -> ApiResult<HashMap<String, RoleCost>> {
    let lines: Vec<CostLine> = sqlx::query_as(
        "SELECT r.slug, l.ingredient_id, l.state, l.qty, l.unit, l.role, l.optional, l.per_batch
         FROM recipe_ingredients l JOIN recipes r ON r.id = l.recipe_id
         WHERE r.household_id = $1 OR (r.household_id IS NULL AND r.retired_at IS NULL)",
    )
    .bind(household_id)
    .fetch_all(db)
    .await?;
    let ids: Vec<Uuid> = lines.iter().map(|l| l.ingredient_id).collect();
    let book = PriceBook::load(db, &ids).await?;
    let mut costs: HashMap<String, RoleCost> = HashMap::new();
    for line in &lines {
        let cost = costs.entry(line.slug.clone()).or_insert_with(|| RoleCost {
            complete: true,
            ..RoleCost::default()
        });
        if line.optional {
            continue;
        }
        let Some(qty) = line.qty else { continue };
        let Some(cents) = cents(&book, line.ingredient_id, line.state, Some(qty), line.unit) else {
            cost.complete = false;
            continue;
        };
        if line.per_batch {
            cost.batch += cents;
        } else {
            cost.roles.push((line.role, cents));
        }
    }
    Ok(costs)
}

/// Les tiers des recettes au prix complet, au prix par portion standard ; moins de trois : pas de tiers.
pub(crate) fn tiers(costs: &HashMap<String, RoleCost>) -> HashMap<String, PriceTier> {
    let mut ranked: Vec<(&String, f64)> = costs
        .iter()
        .filter(|(_, c)| c.complete)
        .map(|(slug, c)| (slug, c.per_portion()))
        .collect();
    if ranked.len() < 3 {
        return HashMap::new();
    }
    ranked.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(b.0)));
    let n = ranked.len();
    ranked
        .into_iter()
        .enumerate()
        .map(|(rank, (slug, _))| {
            let tier = if rank * 3 < n {
                PriceTier::Low
            } else if rank * 3 < 2 * n {
                PriceTier::Mid
            } else {
                PriceTier::High
            };
            (slug.clone(), tier)
        })
        .collect()
}

/// Les portions d'un repas hors menu, selon la présence : au dîner, les boîtes du lendemain midi comprises.
pub(crate) async fn meal_scale(
    db: &PgPool,
    household_id: Uuid,
    date: Day,
    meal: Meal,
) -> ApiResult<Portions> {
    let (_, scale) = menus::presence_scale(db, household_id, date, meal).await?;
    if meal != Meal::Dinner {
        return Ok(scale);
    }
    let (_, boxes) = menus::presence_scale(db, household_id, date.plus(1), Meal::Lunch).await?;
    Ok(Portions {
        protein: scale.protein + boxes.protein,
        starch: scale.starch + boxes.starch,
        veg: scale.veg + boxes.veg,
        fat: scale.fat + boxes.fat,
        other: scale.other + boxes.other,
    })
}

/// Chiffre le menu lu : chaque plat, et la semaine (le total des courses).
pub(crate) async fn price_menu(db: &PgPool, household_id: Uuid, menu: &mut Menu) -> ApiResult<()> {
    let Some(week) = shopping::list(db, household_id, menu.week_start)
        .await?
        .price
    else {
        return Ok(());
    };
    let tiers = tiers(&recipe_costs(db, household_id).await?);
    for (slot, meal) in menu.slots.iter_mut().zip(&week.meals) {
        slot.price = Some(SlotPrice {
            cents: meal.cents,
            tier: tiers
                .get(&slot.recipe.slug)
                .copied()
                .filter(|_| meal.complete),
            complete: meal.complete,
        });
    }
    menu.price = Some(week);
    Ok(())
}

/// Prix estimé d'un repas du menu, boîtes du lendemain comprises.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct MealPrice {
    #[schema(value_type = String, format = Date)]
    pub date: Day,
    pub meal: Meal,
    /// Slug de la recette.
    pub recipe: String,
    pub title: String,
    pub cents: i32,
    /// Toutes ses lignes ont un prix.
    pub complete: bool,
}

/// Prix estimé d'une semaine de courses.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WeekPrice {
    /// Les articles à acheter, aux quantités de la liste, et le placard au prorata.
    pub total_cents: i32,
    pub per_portion_cents: i32,
    /// Portions de tous les repas, boîtes comprises.
    pub portions: i32,
    pub meals: Vec<MealPrice>,
    /// Ce que le total a de plus que la somme des repas : pièces arrondies, articles partagés entre plats.
    pub shared_cents: i32,
    /// Articles du menu sans prix (ils ne comptent pas dans le total).
    pub missing: i32,
    /// Plafond du foyer.
    pub budget_cents: Option<i32>,
    /// Au-dessus du plafond ; nul sans plafond, ou quand des articles n'ont pas de prix (comparaison incomplète).
    pub over_budget: Option<bool>,
    /// Le plus ancien calcul des prix utilisés.
    #[schema(value_type = Option<String>, format = Date)]
    pub updated_on: Option<Day>,
}

/// Prix d'une quantité d'un ingrédient, s'il est connu et que l'unité se convertit.
pub(crate) fn cents(
    book: &PriceBook,
    ingredient_id: Uuid,
    state: IngredientState,
    qty: Option<f32>,
    unit: Unit,
) -> Option<f64> {
    let price = book.prices.get(&(ingredient_id, state))?;
    line_cents(
        qty?,
        unit,
        price,
        book.piece_g.get(&ingredient_id).copied(),
        book.density.get(&ingredient_id).copied(),
    )
}

/// Prix d'un repas : ses lignes à ses portions ; les lignes facultatives et « au goût » ne comptent pas.
pub(crate) fn meal_price(slot: &SlotLines, book: &PriceBook) -> MealPrice {
    let mut total = 0.0;
    let mut complete = true;
    for line in slot.lines.iter().filter(|l| !l.optional) {
        let Some(qty) = line.qty else { continue };
        let qty = if line.per_batch {
            qty
        } else {
            qty * slot.scale.for_role(line.role)
        };
        match cents(book, line.ingredient_id, line.state, Some(qty), line.unit) {
            Some(c) => total += c,
            None => complete = false,
        }
    }
    MealPrice {
        date: slot.date,
        meal: slot.meal,
        recipe: slot.slug.clone(),
        title: slot.title.clone(),
        cents: total.round() as i32,
        complete,
    }
}

/// Prix de la semaine : `items` est le prix de chaque article à acheter (nul s'il n'en a pas), `slots` les repas.
/// Rien à acheter : pas de prix.
pub(crate) fn week_price(
    items: &[Option<f64>],
    slots: &[SlotLines],
    book: &PriceBook,
    budget_cents: Option<i32>,
) -> Option<WeekPrice> {
    if items.is_empty() {
        return None;
    }
    let total_cents: i32 = items.iter().flatten().map(|c| c.round() as i32).sum();
    let missing = items.iter().filter(|c| c.is_none()).count() as i32;
    let meals: Vec<MealPrice> = slots.iter().map(|s| meal_price(s, book)).collect();
    let portions: i32 = slots.iter().map(|s| s.servings).sum();
    let meals_cents: i32 = meals.iter().map(|m| m.cents).sum();
    Some(WeekPrice {
        total_cents,
        per_portion_cents: if portions > 0 {
            (f64::from(total_cents) / f64::from(portions)).round() as i32
        } else {
            0
        },
        portions,
        meals,
        shared_cents: (total_cents - meals_cents).max(0),
        missing,
        budget_cents,
        over_budget: budget_cents
            .filter(|_| missing == 0)
            .map(|b| total_cents > b),
        updated_on: book.updated_on(),
    })
}

/// Ce que coûterait un lot publié, sans rien écrire.
#[derive(Debug, Serialize, ToSchema)]
pub struct Estimate {
    /// Prix de chaque plat du menu du lot, dans l'ordre du menu.
    pub meals: Vec<MealPrice>,
    /// Total de la semaine, comme la liste de courses le donnerait ; nul sans menu ou sans rien à acheter.
    pub week: Option<WeekPrice>,
    /// Codes d'ingrédients ni dans le référentiel ni dans le lot (leurs lignes ne sont pas chiffrées).
    pub unknown: Vec<String>,
}

/// Un ingrédient du lot ou du référentiel, pour faire les lignes.
struct Known {
    id: Uuid,
    name: String,
    aisle: Aisle,
    default_state: IngredientState,
}

/// Chiffre un lot comme si on le publiait : ses recettes (sinon celles du foyer ou de la communauté), ses
/// ingrédients nouveaux chiffrés à la volée, son menu aux portions de la présence.
pub(crate) async fn estimate_lot(
    state: &AppState,
    household_id: Uuid,
    lot: &Lot,
) -> ApiResult<Estimate> {
    let db = &state.db;
    let codes: Vec<String> = lot
        .recipes
        .iter()
        .flat_map(|r| r.ingredients.iter().map(|l| l.ingredient.clone()))
        .collect();
    let rows: Vec<(Uuid, String, String, String, String)> = sqlx::query_as(
        "SELECT id, code, name, aisle, default_state FROM ingredients WHERE code = ANY($1)",
    )
    .bind(&codes)
    .fetch_all(db)
    .await?;
    let mut known: HashMap<String, Known> = rows
        .into_iter()
        .filter_map(|(id, code, name, aisle, state)| {
            Some((
                code,
                Known {
                    id,
                    name,
                    aisle: Aisle::try_from(aisle).ok()?,
                    default_state: IngredientState::try_from(state).ok()?,
                },
            ))
        })
        .collect();

    // Les ingrédients nouveaux du lot : un identifiant provisoire, et un prix tiré de leurs rattachements.
    let now = OffsetDateTime::now_utc();
    let mut fresh_prices: Vec<(Uuid, UnitPrice)> = Vec::new();
    let mut sizes: Vec<(Uuid, Option<f32>, Option<f32>)> = Vec::new();
    let started = std::time::Instant::now();
    let mut fresh = 0;
    for ingredient in &lot.ingredients {
        let name = ingredient.name.trim();
        let ingredient_code = code(name);
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM ingredients WHERE search_key = $1 OR code = $2)",
        )
        .bind(search_key(name))
        .bind(&ingredient_code)
        .fetch_one(db)
        .await?;
        if exists || known.contains_key(&ingredient_code) {
            continue;
        }
        let id = Uuid::now_v7();
        let priceable = fresh < MAX_FRESH && started.elapsed() < FRESH_BUDGET;
        if priceable && !ingredient.price_links.is_empty() {
            fresh += 1;
        }
        for link in ingredient.price_links.iter().filter(|_| priceable) {
            let target = run::Target::new(
                id,
                ingredient_code.clone(),
                ingredient.default_unit,
                ingredient.piece_g,
                ingredient.density,
                link,
            );
            // Une source en panne laisse l'ingrédient sans prix : l'estimation le compte comme tel.
            let left = FRESH_BUDGET.saturating_sub(started.elapsed());
            let priced =
                tokio::time::timeout(left, run::price_one(state, &target, Day(now.date()))).await;
            if let Ok(Ok(Some(crossed))) = priced {
                fresh_prices.push((
                    id,
                    UnitPrice {
                        state: link.state,
                        cents: crossed.cents.max(1),
                        per: target.reference(),
                        source: crossed.source,
                        observations: crossed.observations,
                        observed_on: crossed.observed_on,
                        computed_at: now,
                    },
                ));
            }
        }
        sizes.push((id, ingredient.piece_g, ingredient.density));
        known.insert(
            ingredient_code,
            Known {
                id,
                name: name.to_owned(),
                aisle: ingredient.aisle,
                default_state: ingredient.default_state,
            },
        );
    }

    let Some(menu) = &lot.menu else {
        return Ok(Estimate {
            meals: Vec::new(),
            week: None,
            unknown: Vec::new(),
        });
    };

    // Les lignes : recettes du lot, puis celles du foyer ou de la communauté.
    let mut unknown: Vec<String> = Vec::new();
    let mut lines: HashMap<String, Vec<LineRow>> = HashMap::new();
    let mut titles: HashMap<String, String> = HashMap::new();
    for recipe in &lot.recipes {
        let Some(slug) = recipe.slug.clone() else {
            continue;
        };
        let recipe_id = Uuid::now_v7();
        let mut rows = Vec::new();
        for line in &recipe.ingredients {
            let Some(k) = known.get(&line.ingredient) else {
                unknown.push(line.ingredient.clone());
                continue;
            };
            let (unit, factor) = line.unit.normalize();
            rows.push(LineRow {
                recipe_id,
                slug: slug.clone(),
                title: recipe.title.clone(),
                ingredient_id: k.id,
                code: line.ingredient.clone(),
                name: k.name.clone(),
                aisle: k.aisle,
                state: line.state.unwrap_or(k.default_state),
                qty: line.qty.map(|q| q * factor),
                unit,
                role: line.role,
                optional: line.optional,
                per_batch: line.per_batch,
            });
        }
        titles.insert(slug.clone(), recipe.title.clone());
        lines.insert(slug, rows);
    }
    let elsewhere: Vec<&str> = menu
        .slots
        .iter()
        .map(|s| s.recipe.as_str())
        .filter(|slug| !lines.contains_key(*slug))
        .collect();
    let stored: Vec<LineRow> = sqlx::query_as(
        "SELECT r.id AS recipe_id, r.slug, r.title, l.ingredient_id, i.code, i.name, i.aisle, l.state, l.qty, l.unit, l.role,
                l.optional, l.per_batch
         FROM recipe_ingredients l JOIN recipes r ON r.id = l.recipe_id JOIN ingredients i ON i.id = l.ingredient_id
         WHERE (r.household_id = $1 OR r.household_id IS NULL) AND r.slug = ANY($2) ORDER BY l.position",
    )
    .bind(household_id)
    .bind(&elsewhere)
    .fetch_all(db)
    .await?;
    for line in stored {
        titles.insert(line.slug.clone(), line.title.clone());
        lines.entry(line.slug.clone()).or_default().push(line);
    }

    // Les repas, aux portions que leur donnerait la publication, dans l'ordre du menu publié.
    let members = members::load(db, household_id, None).await?;
    let mut slots = Vec::with_capacity(menu.slots.len());
    for slot in &menu.slots {
        let Some(title) = titles.get(&slot.recipe) else {
            return Err(crate::error::ApiError::invalid(format!(
                "{} : recette inconnue.",
                slot.recipe
            )));
        };
        let (diners, boxes) = menus::slot_counts(&members, slot)?;
        let day = slot.portions_date.unwrap_or(slot.date);
        let (d, b, scale) = menus::slot_scale(
            &members,
            day,
            slot.meal,
            slot.covers_next_lunch,
            diners,
            boxes,
        );
        slots.push(SlotLines {
            date: slot.date,
            meal: slot.meal,
            slug: slot.recipe.clone(),
            title: title.clone(),
            servings: d + b,
            scale,
            lines: lines.get(&slot.recipe).cloned().unwrap_or_default(),
        });
    }
    slots.sort_by_key(|s| (s.date, s.meal == Meal::Dinner));

    let ids: Vec<Uuid> = slots
        .iter()
        .flat_map(|s| s.lines.iter().map(|l| l.ingredient_id))
        .collect();
    let mut book = PriceBook::load(db, &ids).await?;
    for (id, price) in fresh_prices {
        book.prices.insert((id, price.state), price);
    }
    for (id, piece_g, density) in sizes {
        if let Some(g) = piece_g {
            book.piece_g.insert(id, g);
        }
        if let Some(d) = density {
            book.density.insert(id, d);
        }
    }
    let items = shopping::aggregate(&slots);
    let bought = shopping::bought_cents(&items, &book);
    let budget = households::weekly_budget(db, household_id).await?;
    let week = week_price(&bought, &slots, &book, budget);
    unknown.sort();
    unknown.dedup();
    Ok(Estimate {
        meals: slots.iter().map(|s| meal_price(s, &book)).collect(),
        week,
        unknown,
    })
}
