//! Menus du foyer : une période qui commence à son premier repas (`week_start`), des créneaux d'un plat chacun.
//! Les portions d'un créneau suivent la présence des membres (ou un nombre fixé), et un menu se lit avec ses
//! avertissements : conflits d'appareil, matériel absent, exclusions, midi sans boîtes, chevauchements.

use std::collections::{BTreeMap, HashMap, HashSet};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sqlx::{FromRow, PgConnection, PgPool};
use time::{Date, Duration, Month};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    AppState,
    auth::Caller,
    error::{ApiError, ApiResult, ErrorBody},
    feedback::Rating,
    members::{self, Member, Portions},
    nutrition::{self, Nutrition},
    prices::estimate::{self, SlotPrice, WeekPrice},
    recipes::{self, Recipe},
    types::{Aisle, ExclusionKind, IngredientState, Meal, Scope, Unit},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/households/{id}/menus", get(list_menus))
        .route(
            "/households/{id}/menus/{week_start}",
            get(read_menu).put(put_menu).delete(delete_menu),
        )
        .route("/households/{id}/menus/{week_start}/move", post(move_slot))
        .route("/households/{id}/menus/{week_start}/freeze", post(freeze))
        .route("/households/{id}/agenda", get(agenda))
}

/// Un menu s'étend sur quatre semaines au plus à partir de son premier repas (de quoi déplacer des plats).
const MENU_SPAN_DAYS: i64 = 27;
/// Variante express : 15 minutes actives au plus.
const EXPRESS_MAX_MIN: i16 = 15;
const AGENDA_MAX_DAYS: u32 = 28;

// Dates

/// Jour calendaire, `2026-10-18` en JSON.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Day(pub Date);

impl Day {
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.splitn(3, '-');
        let year: i32 = parts.next()?.parse().ok()?;
        let month: u8 = parts.next()?.parse().ok()?;
        let day: u8 = parts.next()?.parse().ok()?;
        if text.len() != 10 {
            return None;
        }
        let month = Month::try_from(month).ok()?;
        Date::from_calendar_date(year, month, day).ok().map(Self)
    }

    pub fn plus(self, days: i64) -> Self {
        Self(self.0 + Duration::days(days))
    }

    fn days_until(self, later: Day) -> i64 {
        (later.0 - self.0).whole_days()
    }

    /// 0 = lundi … 6 = dimanche, comme la présence des membres.
    fn weekday_index(self) -> usize {
        self.0.weekday().number_days_from_monday() as usize
    }

    /// « dimanche 18 octobre ».
    fn french(self) -> String {
        const DAYS: [&str; 7] = [
            "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
        ];
        const MONTHS: [&str; 12] = [
            "janvier",
            "février",
            "mars",
            "avril",
            "mai",
            "juin",
            "juillet",
            "août",
            "septembre",
            "octobre",
            "novembre",
            "décembre",
        ];
        let day = self.0.day();
        let day = if day == 1 {
            "1er".to_owned()
        } else {
            day.to_string()
        };
        format!(
            "{} {day} {}",
            DAYS[self.weekday_index()],
            MONTHS[u8::from(self.0.month()) as usize - 1]
        )
    }
}

impl std::fmt::Display for Day {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02}",
            self.0.year(),
            u8::from(self.0.month()),
            self.0.day()
        )
    }
}

impl Serialize for Day {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Day {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("date AAAA-MM-JJ attendue : {text}")))
    }
}

fn meal_order(meal: Meal) -> u8 {
    match meal {
        Meal::Lunch => 0,
        Meal::Dinner => 1,
    }
}

fn meal_french(meal: Meal) -> &'static str {
    match meal {
        Meal::Lunch => "midi",
        Meal::Dinner => "soir",
    }
}

fn equipment_name(code: &str) -> &str {
    match code {
        "thermomix_tm6" => "Thermomix",
        "cookeo_6l" => "Cookeo",
        "pressure_cooker" => "autocuiseur",
        "oven" => "four",
        "hob" => "plaques",
        "microwave" => "micro-ondes",
        "airfryer" => "airfryer",
        "blender" => "blender",
        other => other,
    }
}

// Saisie

/// Plat prévu à un repas.
#[derive(Deserialize, ToSchema)]
pub struct SlotInput {
    #[schema(value_type = String, format = Date)]
    pub date: Day,
    pub meal: Meal,
    /// Slug d'une recette du foyer.
    pub recipe: String,
    /// Nom de la variante ; absent : la variante par défaut (ou la première dont le foyer a le matériel).
    pub variant: Option<String>,
    /// Le plat fait aussi les boîtes du lendemain midi.
    #[serde(default)]
    pub covers_next_lunch: bool,
    /// Mangeurs du repas, en nombre ; absent : les membres présents.
    pub diners: Option<i32>,
    /// Boîtes du lendemain, en nombre ; absent : les membres présents le lendemain midi.
    pub next_lunch_covers: Option<i32>,
    /// Total des portions (format des lots) : comparé aux portions calculées, seul l'écart est fixé en nombre.
    pub servings: Option<i32>,
    /// Les portions se calculent comme ce jour-là (plat déplacé après les courses).
    #[schema(value_type = Option<String>, format = Date)]
    pub portions_date: Option<Day>,
    pub note: Option<String>,
    /// Ingrédients du plat congelés en rentrant des courses (codes) : rappel la veille de les sortir.
    #[serde(default)]
    pub thaw: Vec<String>,
}

/// Menu complet, tel que publié.
#[derive(Deserialize, ToSchema)]
pub struct MenuInput {
    /// Obligatoire dans un lot ; dans un PUT, doit être celui de l'adresse s'il est donné.
    #[schema(value_type = Option<String>, format = Date)]
    pub week_start: Option<Day>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub notes: String,
    pub slots: Vec<SlotInput>,
}

crate::types::text_enum! {
    /// Que faire du plat qui occupe déjà le repas d'arrivée.
    pub enum MoveMode {
        /// Le plat prend la place ; les suivants glissent d'un jour, jusqu'au premier repas libre.
        Insert = "insert",
        /// Les deux plats changent de jour.
        Swap = "swap",
        /// Les deux plats sont servis ensemble.
        Together = "together",
    }
}

/// Déplacer un plat vers un autre repas du menu.
#[derive(Deserialize, ToSchema)]
pub struct MoveInput {
    /// Créneau à déplacer.
    pub slot: Uuid,
    #[schema(value_type = String, format = Date)]
    pub date: Day,
    pub meal: Meal,
    pub mode: MoveMode,
}

/// Congeler (ou non) un ingrédient d'un plat.
#[derive(Deserialize, ToSchema)]
pub struct FreezeInput {
    pub slot: Uuid,
    /// Code de l'ingrédient.
    pub ingredient: String,
    pub frozen: bool,
}

// Lecture

/// Ingrédient congelé pour ce plat.
#[derive(Serialize, ToSchema)]
pub struct ThawItem {
    pub ingredient: String,
    pub name: String,
}

#[derive(Serialize, ToSchema)]
pub struct RecipeRef {
    pub slug: String,
    pub title: String,
}

/// Variante retenue pour le créneau.
#[derive(Serialize, ToSchema)]
pub struct SlotVariant {
    pub label: String,
    pub equipment: Vec<String>,
    /// Équipements de la variante que le foyer n'a pas.
    pub missing_equipment: Vec<String>,
    pub active_min: i16,
    pub total_min: i16,
    pub vessels: i16,
}

/// Un membre qui mange ce plat : au repas même (`dinner` ou `lunch`), ou en boîte le lendemain midi (`box`).
#[derive(Serialize, ToSchema)]
pub struct SlotEater {
    pub member_id: Uuid,
    pub name: String,
    /// `meal` : au repas ; `box` : boîte du lendemain.
    pub at: &'static str,
    pub portions: Portions,
}

/// Plat prévu, avec ses portions.
#[derive(Serialize, ToSchema)]
pub struct MenuSlot {
    pub id: Uuid,
    /// Menu du créneau.
    #[schema(value_type = String, format = Date)]
    pub week_start: Day,
    #[schema(value_type = String, format = Date)]
    pub date: Day,
    pub meal: Meal,
    /// Rang du plat dans le repas (deux plats le même soir).
    pub position: i16,
    pub recipe: RecipeRef,
    pub variant: SlotVariant,
    pub covers_next_lunch: bool,
    pub diners: i32,
    /// `diners` est un nombre fixé, et non les membres présents.
    pub diners_fixed: bool,
    pub next_lunch_covers: i32,
    pub next_lunch_covers_fixed: bool,
    /// Portions du plat : `diners` + `next_lunch_covers`.
    pub servings: i32,
    /// Multiplicateur de chaque rôle d'ingrédient : la somme des barèmes de ceux qui mangent (au prorata quand un
    /// nombre est fixé). Quantité d'une ligne = portion standard × `scale[rôle]`.
    pub scale: Portions,
    /// Les membres dont le barème fait `scale`.
    pub eaters: Vec<SlotEater>,
    /// Variante de 15 minutes actives au plus.
    pub express: bool,
    /// Jours d'avance d'une phase faite avant (1 : la veille).
    pub prep_days_before: i16,
    /// Portions calculées comme ce jour-là (plat déplacé).
    #[schema(value_type = Option<String>, format = Date)]
    pub portions_date: Option<Day>,
    pub note: Option<String>,
    /// Ingrédients congelés pour ce plat : à sortir la veille.
    pub thaw: Vec<ThawItem>,
    /// « Comment c'était ? », une fois le repas noté.
    pub rating: Option<Rating>,
    /// Prix estimé du plat ; rempli à la lecture du menu.
    pub price: Option<SlotPrice>,
    /// Apports de ta portion et note de la recette ; remplis à la lecture du menu.
    pub nutrition: Option<Nutrition>,
}

crate::types::text_enum! {
    /// Nature d'un avertissement.
    pub enum WarningKind {
        /// Deux plats du même repas demandent le même appareil exclusif (Cookeo, Thermomix…).
        EquipmentConflict = "equipment_conflict",
        /// La variante demande un appareil que le foyer n'a pas.
        MissingEquipment = "missing_equipment",
        /// Un mangeur a une allergie ou un dégoût sur un ingrédient du plat.
        Exclusion = "exclusion",
        /// Un plat déplacé faisait les boîtes de ce midi.
        LunchWithoutBoxes = "lunch_without_boxes",
        /// Un autre menu prévoit aussi ce repas.
        Overlap = "overlap",
        /// La variante prévue n'existe plus : la variante par défaut la remplace.
        UnknownVariant = "unknown_variant",
        /// Un ingrédient frais ne tiendra pas jusqu'au jour du plat : le congeler, ou avancer le plat.
        Freshness = "freshness",
        /// Le total estimé de la semaine dépasse le plafond du foyer.
        Budget = "budget",
    }
}

/// Autre variante qui lève un conflit.
#[derive(Serialize, ToSchema)]
pub struct Suggestion {
    pub slot: Uuid,
    pub variant: String,
}

#[derive(Serialize, ToSchema)]
pub struct Warning {
    pub kind: WarningKind,
    #[schema(value_type = String, format = Date)]
    pub date: Day,
    pub meal: Meal,
    /// Créneaux concernés.
    pub slots: Vec<Uuid>,
    pub equipment: Vec<String>,
    pub member: Option<String>,
    /// Code de l'ingrédient.
    pub ingredient: Option<String>,
    /// Nom de l'ingrédient, affichable.
    pub ingredient_name: Option<String>,
    pub exclusion: Option<ExclusionKind>,
    #[schema(value_type = Option<String>, format = Date)]
    pub other_menu: Option<Day>,
    pub suggestion: Option<Suggestion>,
    /// Fraîcheur : jour d'achat (case cochée, ou jour habituel des courses projeté).
    #[schema(value_type = Option<String>, format = Date)]
    pub purchased: Option<Day>,
    /// Fraîcheur : jours que l'ingrédient tient au réfrigérateur.
    pub keeps_days: Option<i16>,
    /// Fraîcheur : dernier jour où le plat serait encore frais.
    #[schema(value_type = Option<String>, format = Date)]
    pub advance_to: Option<Day>,
    /// En français, affichable tel quel.
    pub message: String,
}

/// Menu complet, avec portions et avertissements.
#[derive(Serialize, ToSchema)]
pub struct Menu {
    #[schema(value_type = String, format = Date)]
    pub week_start: Day,
    pub title: String,
    pub notes: String,
    /// Par date, repas (midi puis soir) et rang.
    pub slots: Vec<MenuSlot>,
    pub warnings: Vec<Warning>,
    /// Prix estimé de la semaine (le total des courses) ; rempli à la lecture du menu.
    pub price: Option<WeekPrice>,
}

/// Menu, dans l'historique.
#[derive(Serialize, ToSchema)]
pub struct MenuSummary {
    #[schema(value_type = String, format = Date)]
    pub week_start: Day,
    pub title: String,
    #[schema(value_type = Option<String>, format = Date)]
    pub first_date: Option<Day>,
    #[schema(value_type = Option<String>, format = Date)]
    pub last_date: Option<Day>,
    pub slot_count: i64,
}

/// Les repas d'une période, tous menus confondus, avec les recettes pour lire et cuisiner sans autre appel.
#[derive(Serialize, ToSchema)]
pub struct Agenda {
    #[schema(value_type = String, format = Date)]
    pub from: Day,
    pub days: u32,
    pub slots: Vec<MenuSlot>,
    pub recipes: Vec<Recipe>,
    pub warnings: Vec<Warning>,
}

// Base

#[derive(FromRow)]
struct MenuRow {
    id: Uuid,
    week_start: Date,
    title: String,
    notes: String,
}

#[derive(Clone, FromRow)]
struct SlotRow {
    id: Uuid,
    date: Date,
    #[sqlx(try_from = "String")]
    meal: Meal,
    position: i16,
    recipe_id: Uuid,
    variant_label: Option<String>,
    covers_next_lunch: bool,
    diners: Option<i16>,
    next_lunch_covers: Option<i16>,
    portions_date: Option<Date>,
    note: Option<String>,
    thaw: Vec<String>,
}

impl SlotRow {
    fn day(&self) -> Day {
        Day(self.date)
    }

    fn key(&self) -> (Day, u8, i16) {
        (self.day(), meal_order(self.meal), self.position)
    }
}

/// Ce qu'il faut savoir d'une recette pour planifier : variantes et ingrédients.
struct RecipeBrief {
    slug: String,
    title: String,
    variants: Vec<VariantBrief>,
    ingredients: Vec<Ingredient>,
}

/// Ligne de recette, pour les exclusions et la fraîcheur.
struct Ingredient {
    id: Uuid,
    code: String,
    name: String,
    /// Clé de l'article de courses (`{id}:{état}:{unité}`).
    key: String,
    /// Jours au réfrigérateur, pour un ingrédient frais ; `None` : pas de souci (surgelé, sec, épicerie…).
    keeps: Option<i16>,
}

/// Jours au réfrigérateur d'un ingrédient frais : le sien, sinon celui de son rayon.
fn keeps_days(state: IngredientState, aisle: Aisle, own: Option<i16>) -> Option<i16> {
    if !matches!(state, IngredientState::Fresh | IngredientState::Cooked) {
        return None;
    }
    own.or(match aisle {
        Aisle::Fish => Some(2),
        Aisle::Butcher | Aisle::Bakery => Some(3),
        Aisle::Produce | Aisle::Dairy | Aisle::Fresh => Some(7),
        Aisle::Grocery | Aisle::Frozen | Aisle::Other => None,
    })
}

#[derive(Clone, FromRow)]
struct VariantBrief {
    recipe_id: Uuid,
    label: String,
    equipment_codes: Vec<String>,
    active_min: i16,
    total_min: i16,
    vessels: i16,
    prep_days_before: i16,
}

/// Le foyer, pour calculer : membres, matériel, appareils exclusifs, recettes des créneaux.
struct Context {
    members: Vec<Member>,
    owned: HashSet<String>,
    exclusive: HashSet<String>,
    recipes: HashMap<Uuid, RecipeBrief>,
    /// Repas notés : (recette, jour, repas).
    ratings: HashMap<(Uuid, Date, Meal), Rating>,
}

async fn context(db: &PgPool, household_id: Uuid, recipe_ids: &[Uuid]) -> ApiResult<Context> {
    let members = members::load(db, household_id, None).await?;
    let owned = sqlx::query_scalar(
        "SELECT equipment_code FROM household_equipment WHERE household_id = $1",
    )
    .bind(household_id)
    .fetch_all(db)
    .await?
    .into_iter()
    .collect();
    let exclusive =
        sqlx::query_scalar("SELECT code FROM equipment WHERE (rules->>'exclusive')::boolean")
            .fetch_all(db)
            .await?
            .into_iter()
            .collect();
    let mut recipes: HashMap<Uuid, RecipeBrief> = sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT id, slug, title FROM recipes WHERE id = ANY($1)",
    )
    .bind(recipe_ids)
    .fetch_all(db)
    .await?
    .into_iter()
    .map(|(id, slug, title)| {
        (
            id,
            RecipeBrief {
                slug,
                title,
                variants: Vec::new(),
                ingredients: Vec::new(),
            },
        )
    })
    .collect();
    let variants: Vec<VariantBrief> = sqlx::query_as(
        "SELECT v.recipe_id, v.label, v.equipment_codes, v.active_min, v.total_min, v.vessels,
                coalesce((SELECT -min(p.day_offset) FROM variant_phases p WHERE p.variant_id = v.id), 0)::smallint
                    AS prep_days_before
         FROM recipe_variants v WHERE v.recipe_id = ANY($1) ORDER BY v.is_default DESC, v.position",
    )
    .bind(recipe_ids)
    .fetch_all(db)
    .await?;
    for v in variants {
        if let Some(r) = recipes.get_mut(&v.recipe_id) {
            r.variants.push(v);
        }
    }
    type Line = (
        Uuid,
        Uuid,
        String,
        String,
        String,
        String,
        String,
        Option<i16>,
    );
    let lines: Vec<Line> = sqlx::query_as(
        "SELECT l.recipe_id, i.id, i.code, i.name, l.state, l.unit, i.aisle, i.keeps_days
         FROM recipe_ingredients l JOIN ingredients i ON i.id = l.ingredient_id
         WHERE l.recipe_id = ANY($1) ORDER BY l.position",
    )
    .bind(recipe_ids)
    .fetch_all(db)
    .await?;
    for (recipe_id, id, code, name, state, unit, aisle, own) in lines {
        let (Ok(state), Ok(aisle), Ok(unit)) = (
            IngredientState::try_from(state),
            Aisle::try_from(aisle),
            Unit::try_from(unit),
        ) else {
            continue;
        };
        if let Some(r) = recipes.get_mut(&recipe_id) {
            r.ingredients.push(Ingredient {
                key: format!("{id}:{}:{}", state.as_str(), unit.as_str()),
                keeps: keeps_days(state, aisle, own),
                id,
                code,
                name,
            });
        }
    }
    let ratings = sqlx::query_as::<_, (Uuid, Date, String, String)>(
        "SELECT recipe_id, date, meal, rating FROM meal_ratings WHERE household_id = $1 AND recipe_id = ANY($2)",
    )
    .bind(household_id)
    .bind(recipe_ids)
    .fetch_all(db)
    .await?
    .into_iter()
    .filter_map(|(recipe, date, meal, rating)| {
        Some((
            (recipe, date, Meal::try_from(meal).ok()?),
            Rating::try_from(rating).ok()?,
        ))
    })
    .collect();
    Ok(Context {
        members,
        owned,
        exclusive,
        recipes,
        ratings,
    })
}

async fn menu_row(db: &PgPool, household_id: Uuid, week_start: Day) -> ApiResult<MenuRow> {
    sqlx::query_as("SELECT id, week_start, title, notes FROM menus WHERE household_id = $1 AND week_start = $2")
        .bind(household_id)
        .bind(week_start.0)
        .fetch_optional(db)
        .await?
        .ok_or_else(ApiError::not_found)
}

macro_rules! slot_columns {
    () => {
        "s.id, s.date, s.meal, s.position, s.recipe_id, s.variant_label, s.covers_next_lunch, s.diners,
         s.next_lunch_covers, s.portions_date, s.note, s.thaw"
    };
}

async fn slot_rows(db: &PgPool, menu_ids: &[Uuid]) -> ApiResult<Vec<SlotRow>> {
    let mut rows: Vec<SlotRow> = sqlx::query_as(concat!(
        "SELECT ",
        slot_columns!(),
        " FROM menu_slots s WHERE s.menu_id = ANY($1)"
    ))
    .bind(menu_ids)
    .fetch_all(db)
    .await?;
    rows.sort_by_key(SlotRow::key);
    Ok(rows)
}

/// Repas que d'autres menus du foyer prévoient aussi : (date, repas) → `week_start` des autres menus.
async fn overlaps(
    db: &PgPool,
    household_id: Uuid,
    menu_id: Uuid,
    rows: &[SlotRow],
) -> ApiResult<HashMap<(Day, Meal), Vec<Day>>> {
    let dates: Vec<Date> = rows.iter().map(|r| r.date).collect();
    let others: Vec<(Date, String, Date)> = sqlx::query_as(
        "SELECT DISTINCT s.date, s.meal, m.week_start FROM menu_slots s JOIN menus m ON m.id = s.menu_id
         WHERE m.household_id = $1 AND m.id <> $2 AND s.date = ANY($3) ORDER BY m.week_start",
    )
    .bind(household_id)
    .bind(menu_id)
    .bind(&dates)
    .fetch_all(db)
    .await?;
    let mut map: HashMap<(Day, Meal), Vec<Day>> = HashMap::new();
    for (date, meal, week_start) in others {
        if let Ok(meal) = Meal::try_from(meal) {
            map.entry((Day(date), meal))
                .or_default()
                .push(Day(week_start));
        }
    }
    Ok(map)
}

// Calcul

fn add(a: Portions, b: Portions) -> Portions {
    Portions {
        protein: a.protein + b.protein,
        starch: a.starch + b.starch,
        veg: a.veg + b.veg,
        fat: a.fat + b.fat,
        other: a.other + b.other,
    }
}

fn times(a: Portions, k: f32) -> Portions {
    Portions {
        protein: a.protein * k,
        starch: a.starch * k,
        veg: a.veg * k,
        fat: a.fat * k,
        other: a.other * k,
    }
}

fn uniform(k: f32) -> Portions {
    Portions {
        protein: k,
        starch: k,
        veg: k,
        fat: k,
        other: k,
    }
}

fn present(members: &[Member], day: Day, meal: Meal) -> Vec<&Member> {
    let index = day.weekday_index();
    members
        .iter()
        .filter(|m| {
            let days = match meal {
                Meal::Lunch => &m.presence.lunch,
                Meal::Dinner => &m.presence.dinner,
            };
            days.get(index).copied().unwrap_or(true)
        })
        .collect()
}

/// Nombre et barème d'un groupe de mangeurs : les présents, ou un nombre fixé au prorata de leur mélange (d'un
/// adulte standard si personne n'est présent).
fn part(mix: &[&Member], fixed: Option<i32>) -> (i32, Portions) {
    let count = fixed.unwrap_or(mix.len() as i32);
    let sum = mix.iter().fold(uniform(0.0), |acc, m| add(acc, m.portions));
    let scale = if mix.is_empty() {
        uniform(count as f32)
    } else {
        times(sum, count as f32 / mix.len() as f32)
    };
    (count, scale)
}

/// Mangeurs et boîtes fixés d'un créneau publié : ceux donnés, ou l'écart entre `servings` (format des lots) et la
/// présence ; nuls : selon la présence.
pub(crate) fn slot_counts(
    members: &[Member],
    slot: &SlotInput,
) -> ApiResult<(Option<i16>, Option<i16>)> {
    let mut diners = count(slot.diners, "Mangeurs")?;
    let mut boxes = count(slot.next_lunch_covers, "Boîtes du lendemain")?;
    let servings = count(slot.servings, "Portions")?;
    if let Some(servings) = servings.map(i32::from)
        && diners.is_none()
        && boxes.is_none()
    {
        let day = slot.portions_date.unwrap_or(slot.date);
        let (d, b) = presence_counts(members, day, slot.meal, slot.covers_next_lunch);
        if servings != d + b {
            if !slot.covers_next_lunch {
                diners = Some(servings as i16);
            } else if servings >= d {
                boxes = Some((servings - d) as i16);
            } else {
                diners = Some(servings as i16);
                boxes = Some(0);
            }
        }
    }
    Ok((diners, boxes))
}

/// Mangeurs, boîtes et barème d'un repas, depuis la présence ou des nombres fixés (comme `slot_view`).
pub(crate) fn slot_scale(
    members: &[Member],
    day: Day,
    meal: Meal,
    covers: bool,
    diners: Option<i16>,
    boxes: Option<i16>,
) -> (i32, i32, Portions) {
    let (d, diners_scale) = part(&present(members, day, meal), diners.map(i32::from));
    let (b, boxes_scale) = if covers {
        part(
            &present(members, day.plus(1), Meal::Lunch),
            boxes.map(i32::from),
        )
    } else {
        (0, uniform(0.0))
    };
    (d, b, add(diners_scale, boxes_scale))
}

/// Portions calculées depuis la présence : mangeurs du repas et boîtes du lendemain.
fn presence_counts(members: &[Member], day: Day, meal: Meal, covers: bool) -> (i32, i32) {
    let diners = present(members, day, meal).len() as i32;
    let boxes = if covers {
        present(members, day.plus(1), Meal::Lunch).len() as i32
    } else {
        0
    };
    (diners, boxes)
}

/// Variante du créneau : celle nommée, sinon la variante par défaut si le foyer a son matériel, sinon la première
/// dont il a tout. Renvoie aussi `true` si le nom prévu n'existe plus.
fn resolve<'a>(
    recipe: &'a RecipeBrief,
    label: Option<&str>,
    owned: &HashSet<String>,
) -> (&'a VariantBrief, bool) {
    if let Some(label) = label
        && let Some(v) = recipe.variants.iter().find(|v| v.label == label)
    {
        return (v, false);
    }
    let fits = |v: &&VariantBrief| v.equipment_codes.iter().all(|c| owned.contains(c));
    let chosen = recipe
        .variants
        .first()
        .filter(fits)
        .or_else(|| recipe.variants.iter().find(fits))
        .or(recipe.variants.first())
        .expect("une recette a au moins une variante");
    (chosen, label.is_some())
}

fn slot_view(row: &SlotRow, week_start: Day, ctx: &Context) -> MenuSlot {
    let recipe = &ctx.recipes[&row.recipe_id];
    let (variant, _) = resolve(recipe, row.variant_label.as_deref(), &ctx.owned);
    let portions_day = row.portions_date.map(Day).unwrap_or(row.day());
    let diners_mix = present(&ctx.members, portions_day, row.meal);
    let boxes_mix = if row.covers_next_lunch {
        present(&ctx.members, portions_day.plus(1), Meal::Lunch)
    } else {
        Vec::new()
    };
    let (diners, diners_scale) = part(&diners_mix, row.diners.map(i32::from));
    let (boxes, boxes_scale) = if row.covers_next_lunch {
        part(&boxes_mix, row.next_lunch_covers.map(i32::from))
    } else {
        (0, uniform(0.0))
    };
    let eaters = diners_mix
        .iter()
        .map(|m| (m, "meal"))
        .chain(boxes_mix.iter().map(|m| (m, "box")))
        .map(|(m, at)| SlotEater {
            member_id: m.id,
            name: m.name.clone(),
            at,
            portions: m.portions,
        })
        .collect();
    MenuSlot {
        id: row.id,
        week_start,
        date: row.day(),
        meal: row.meal,
        position: row.position,
        recipe: RecipeRef {
            slug: recipe.slug.clone(),
            title: recipe.title.clone(),
        },
        variant: SlotVariant {
            label: variant.label.clone(),
            equipment: variant.equipment_codes.clone(),
            missing_equipment: variant
                .equipment_codes
                .iter()
                .filter(|c| !ctx.owned.contains(*c))
                .cloned()
                .collect(),
            active_min: variant.active_min,
            total_min: variant.total_min,
            vessels: variant.vessels,
        },
        covers_next_lunch: row.covers_next_lunch,
        diners,
        diners_fixed: row.diners.is_some(),
        next_lunch_covers: boxes,
        next_lunch_covers_fixed: row.covers_next_lunch && row.next_lunch_covers.is_some(),
        servings: diners + boxes,
        scale: add(diners_scale, boxes_scale),
        eaters,
        express: variant.active_min <= EXPRESS_MAX_MIN,
        prep_days_before: variant.prep_days_before,
        portions_date: row.portions_date.map(Day),
        note: row.note.clone(),
        thaw: row
            .thaw
            .iter()
            .map(|code| ThawItem {
                ingredient: code.clone(),
                name: recipe
                    .ingredients
                    .iter()
                    .find(|i| &i.code == code)
                    .map(|i| i.name.clone())
                    .unwrap_or_else(|| code.clone()),
            })
            .collect(),
        rating: ctx
            .ratings
            .get(&(row.recipe_id, row.date, row.meal))
            .copied(),
        price: None,
        nutrition: None,
    }
}

fn warning(kind: WarningKind, date: Day, meal: Meal, slots: Vec<Uuid>, message: String) -> Warning {
    Warning {
        kind,
        date,
        meal,
        slots,
        equipment: Vec::new(),
        member: None,
        ingredient: None,
        ingredient_name: None,
        exclusion: None,
        other_menu: None,
        suggestion: None,
        purchased: None,
        keeps_days: None,
        advance_to: None,
        message,
    }
}

/// Jour d'achat des ingrédients d'un menu.
struct Purchases {
    /// Case cochée : le jour du geste, par clé d'article.
    checked: HashMap<String, Day>,
    /// Sinon : le jour habituel des courses avant le premier repas, ou la veille de ce repas.
    projected: Day,
}

fn warnings(
    rows: &[SlotRow],
    ctx: &Context,
    overlaps: &HashMap<(Day, Meal), Vec<Day>>,
    purchases: &Purchases,
    week_start: Day,
) -> Vec<Warning> {
    let mut out = Vec::new();
    let mut meals: BTreeMap<(Day, u8), Vec<&SlotRow>> = BTreeMap::new();
    for row in rows {
        meals
            .entry((row.day(), meal_order(row.meal)))
            .or_default()
            .push(row);
    }
    for ((day, _), group) in &meals {
        let meal = group[0].meal;
        let when = format!("{} {}", day.french(), meal_french(meal));
        let chosen: Vec<(&SlotRow, &RecipeBrief, &VariantBrief)> = group
            .iter()
            .map(|row| {
                let recipe = &ctx.recipes[&row.recipe_id];
                let (variant, _) = resolve(recipe, row.variant_label.as_deref(), &ctx.owned);
                (*row, recipe, variant)
            })
            .collect();

        if let Some(others) = overlaps.get(&(*day, meal)) {
            for other in others {
                let mut w = warning(
                    WarningKind::Overlap,
                    *day,
                    meal,
                    group.iter().map(|r| r.id).collect(),
                    format!(
                        "{when} : le menu du {} prévoit aussi ce repas.",
                        other.french()
                    ),
                );
                w.other_menu = Some(*other);
                out.push(w);
            }
        }

        // Conflits d'appareil exclusif.
        let mut codes: Vec<&String> = chosen
            .iter()
            .flat_map(|(_, _, v)| v.equipment_codes.iter())
            .filter(|c| ctx.exclusive.contains(*c))
            .collect();
        codes.sort();
        codes.dedup();
        for code in codes {
            let using: Vec<&(&SlotRow, &RecipeBrief, &VariantBrief)> = chosen
                .iter()
                .filter(|(_, _, v)| v.equipment_codes.contains(code))
                .collect();
            if using.len() < 2 {
                continue;
            }
            // Une autre variante, avec le matériel du foyer, sans cet appareil.
            let suggestion = using.iter().find_map(|(row, recipe, _)| {
                recipe
                    .variants
                    .iter()
                    .find(|v| {
                        !v.equipment_codes.contains(code)
                            && v.equipment_codes.iter().all(|c| ctx.owned.contains(c))
                    })
                    .map(|v| (row.id, recipe.title.as_str(), v.label.clone()))
            });
            let titles: Vec<&str> = using.iter().map(|(_, r, _)| r.title.as_str()).collect();
            let mut message = format!(
                "{when} : deux plats au {} ({}).",
                equipment_name(code),
                titles.join(", ")
            );
            if let Some((_, title, label)) = &suggestion {
                message.push_str(&format!(" {title} peut se faire en variante « {label} »."));
            }
            let mut w = warning(
                WarningKind::EquipmentConflict,
                *day,
                meal,
                using.iter().map(|(row, _, _)| row.id).collect(),
                message,
            );
            w.equipment = vec![code.clone()];
            w.suggestion = suggestion.map(|(slot, _, variant)| Suggestion { slot, variant });
            out.push(w);
        }

        for (row, recipe, variant) in &chosen {
            if row.variant_label.is_some()
                && resolve(recipe, row.variant_label.as_deref(), &ctx.owned).1
            {
                out.push(warning(
                    WarningKind::UnknownVariant,
                    *day,
                    meal,
                    vec![row.id],
                    format!(
                        "{when} : {} n'a plus de variante « {} » ; « {} » la remplace.",
                        recipe.title,
                        row.variant_label.as_deref().unwrap_or_default(),
                        variant.label
                    ),
                ));
            }
            // Exclusions des mangeurs (au repas et en boîte).
            let portions_day = row.portions_date.map(Day).unwrap_or(row.day());
            let mut eaters = present(&ctx.members, portions_day, row.meal);
            if row.covers_next_lunch {
                for m in present(&ctx.members, portions_day.plus(1), Meal::Lunch) {
                    if !eaters.iter().any(|e| e.id == m.id) {
                        eaters.push(m);
                    }
                }
            }
            for member in eaters {
                for exclusion in &member.exclusions {
                    if let Some(Ingredient { code, name, .. }) = recipe
                        .ingredients
                        .iter()
                        .find(|i| i.id == exclusion.ingredient_id)
                    {
                        let what = match exclusion.kind {
                            ExclusionKind::Allergy => "allergie",
                            ExclusionKind::Dislike => "n'aime pas",
                        };
                        let mut w = warning(
                            WarningKind::Exclusion,
                            *day,
                            meal,
                            vec![row.id],
                            format!(
                                "{when} : {} ({what}) : {} dans {}.",
                                member.name,
                                name.to_lowercase(),
                                recipe.title
                            ),
                        );
                        w.member = Some(member.name.clone());
                        w.ingredient = Some(code.clone());
                        w.ingredient_name = Some(name.clone());
                        w.exclusion = Some(exclusion.kind);
                        out.push(w);
                    }
                }
            }
            let missing: Vec<String> = variant
                .equipment_codes
                .iter()
                .filter(|c| !ctx.owned.contains(*c))
                .cloned()
                .collect();
            if !missing.is_empty() {
                let names: Vec<&str> = missing.iter().map(|c| equipment_name(c)).collect();
                let mut w = warning(
                    WarningKind::MissingEquipment,
                    *day,
                    meal,
                    vec![row.id],
                    format!(
                        "{when} : {} demande {}, que le foyer n'a pas.",
                        recipe.title,
                        names.join(", ")
                    ),
                );
                w.equipment = missing;
                out.push(w);
            }
        }
    }

    // Un plat déplacé faisait les boîtes du lendemain de son jour d'origine.
    let mut seen = HashSet::new();
    for row in rows {
        let Some(origin) = row.portions_date.map(Day) else {
            continue;
        };
        if !row.covers_next_lunch || origin == row.day() {
            continue;
        }
        let lunch = origin.plus(1);
        let covered = rows.iter().any(|r| {
            (r.day() == origin && r.meal == Meal::Dinner && r.covers_next_lunch)
                || (r.day() == lunch && r.meal == Meal::Lunch)
        });
        if covered || present(&ctx.members, lunch, Meal::Lunch).is_empty() || !seen.insert(lunch) {
            continue;
        }
        let recipe = &ctx.recipes[&row.recipe_id];
        out.push(warning(
            WarningKind::LunchWithoutBoxes,
            lunch,
            Meal::Lunch,
            vec![row.id],
            format!(
                "{} midi : pas de boîtes, le dîner du {} ({}) est déplacé au {}.",
                lunch.french(),
                origin.french(),
                recipe.title,
                row.day().french()
            ),
        ));
    }
    // Fraîcheur : un ingrédient frais acheté trop tôt pour le jour du plat (ou de sa phase de la veille).
    for row in rows {
        let recipe = &ctx.recipes[&row.recipe_id];
        let (variant, _) = resolve(recipe, row.variant_label.as_deref(), &ctx.owned);
        let cooked = row.day().plus(-i64::from(variant.prep_days_before));
        let mut seen = HashSet::new();
        for line in &recipe.ingredients {
            let Some(keeps) = line.keeps else { continue };
            if row.thaw.contains(&line.code) || !seen.insert(&line.code) {
                continue;
            }
            let bought = purchases
                .checked
                .get(&line.key)
                .copied()
                .unwrap_or(purchases.projected);
            let days = bought.days_until(cooked);
            if days <= i64::from(keeps) {
                continue;
            }
            let advance = bought.plus(i64::from(keeps)).max(week_start);
            let mut w = warning(
                WarningKind::Freshness,
                row.day(),
                row.meal,
                vec![row.id],
                format!(
                    "{} {} : {} acheté le {}, cuisiné le {} : {days} jours, il tient {keeps} jour{}. À congeler en rentrant, ou avancer le plat au {}.",
                    row.day().french(),
                    meal_french(row.meal),
                    line.name,
                    bought.french(),
                    cooked.french(),
                    if keeps > 1 { "s" } else { "" },
                    advance.french()
                ),
            );
            w.ingredient = Some(line.code.clone());
            w.ingredient_name = Some(line.name.clone());
            w.purchased = Some(bought);
            w.keeps_days = Some(keeps);
            w.advance_to = Some(advance);
            out.push(w);
        }
    }
    out.sort_by_key(|w| (w.date, meal_order(w.meal)));
    out
}

/// Jour d'achat : cases cochées du menu, et jour habituel des courses du foyer (le plus fréquent sur 90 jours).
async fn purchases(
    db: &PgPool,
    household_id: Uuid,
    menu_id: Uuid,
    first_meal: Day,
) -> ApiResult<Purchases> {
    let checked: Vec<(String, Date)> = sqlx::query_as(
        "SELECT s.key, (s.checked_at AT TIME ZONE h.timezone)::date
         FROM shopping_items s JOIN menus m ON m.id = s.menu_id JOIN households h ON h.id = m.household_id
         WHERE s.menu_id = $1 AND s.checked AND NOT s.manual AND s.checked_at IS NOT NULL",
    )
    .bind(menu_id)
    .fetch_all(db)
    .await?;
    let usual: Option<i32> = sqlx::query_scalar(
        "SELECT extract(isodow FROM s.checked_at AT TIME ZONE h.timezone)::int AS dow
         FROM shopping_items s JOIN menus m ON m.id = s.menu_id JOIN households h ON h.id = m.household_id
         WHERE m.household_id = $1 AND s.checked AND s.checked_at > now() - interval '90 days'
         GROUP BY dow ORDER BY count(*) DESC, dow LIMIT 1",
    )
    .bind(household_id)
    .fetch_optional(db)
    .await?;
    let projected = match usual {
        Some(dow) => {
            let first = i64::from(first_meal.0.weekday().number_from_monday());
            first_meal.plus(-((first - i64::from(dow)).rem_euclid(7)))
        }
        None => first_meal.plus(-1),
    };
    Ok(Purchases {
        checked: checked
            .into_iter()
            .map(|(key, date)| (key, Day(date)))
            .collect(),
        projected,
    })
}

async fn compute(db: &PgPool, household_id: Uuid, menu: MenuRow) -> ApiResult<Menu> {
    let rows = slot_rows(db, &[menu.id]).await?;
    let recipe_ids: Vec<Uuid> = rows.iter().map(|r| r.recipe_id).collect();
    let ctx = context(db, household_id, &recipe_ids).await?;
    let overlaps = overlaps(db, household_id, menu.id, &rows).await?;
    let week_start = Day(menu.week_start);
    let first_meal = rows.first().map(SlotRow::day).unwrap_or(week_start);
    let purchases = purchases(db, household_id, menu.id, first_meal).await?;
    Ok(Menu {
        week_start,
        title: menu.title,
        notes: menu.notes,
        slots: rows
            .iter()
            .map(|r| slot_view(r, week_start, &ctx))
            .collect(),
        warnings: warnings(&rows, &ctx, &overlaps, &purchases, week_start),
        price: None,
    })
}

/// Le menu tel que l'API le rend : chiffré, ses apports pour qui le lit, avec l'avertissement de plafond s'il est dépassé.
async fn read_priced(
    db: &PgPool,
    household_id: Uuid,
    week_start: Day,
    user: Option<Uuid>,
) -> ApiResult<Menu> {
    let mut menu = read(db, household_id, week_start).await?;
    estimate::price_menu(db, household_id, &mut menu).await?;
    nutrition::nutrition_slots(db, household_id, user, &mut menu.slots).await?;
    if let Some(price) = menu.price.as_ref().filter(|p| p.over_budget == Some(true))
        && let (Some(first), Some(budget)) = (menu.slots.first(), price.budget_cents)
    {
        let euros = |cents: i32| (f64::from(cents) / 100.0).round();
        let message = format!(
            "≈ {} €, au-dessus de votre plafond de {} €.",
            euros(price.total_cents),
            euros(budget)
        );
        menu.warnings.push(warning(
            WarningKind::Budget,
            first.date,
            first.meal,
            Vec::new(),
            message,
        ));
    }
    Ok(menu)
}

pub async fn read(db: &PgPool, household_id: Uuid, week_start: Day) -> ApiResult<Menu> {
    let menu = menu_row(db, household_id, week_start).await?;
    compute(db, household_id, menu).await
}

/// Le créneau d'un repas qui sert cette recette (le menu le plus récent s'il y en a deux) : pour la cuisson en cours,
/// qui ne garde pas l'identifiant du créneau (republier un menu recrée ses créneaux).
pub async fn slot_for(
    db: &PgPool,
    household_id: Uuid,
    date: Day,
    meal: Meal,
    recipe_id: Uuid,
) -> ApiResult<Option<MenuSlot>> {
    let menu: Option<MenuRow> = sqlx::query_as(
        "SELECT m.id, m.week_start, m.title, m.notes FROM menus m
         WHERE m.household_id = $1 AND EXISTS (
             SELECT 1 FROM menu_slots s WHERE s.menu_id = m.id AND s.date = $2 AND s.meal = $3 AND s.recipe_id = $4)
         ORDER BY m.week_start DESC LIMIT 1",
    )
    .bind(household_id)
    .bind(date.0)
    .bind(meal.as_str())
    .bind(recipe_id)
    .fetch_optional(db)
    .await?;
    let Some(menu) = menu else { return Ok(None) };
    let rows: Vec<SlotRow> = slot_rows(db, &[menu.id])
        .await?
        .into_iter()
        .filter(|r| r.day() == date && r.meal == meal && r.recipe_id == recipe_id)
        .collect();
    let ctx = context(db, household_id, &[recipe_id]).await?;
    Ok(rows
        .first()
        .map(|row| slot_view(row, Day(menu.week_start), &ctx)))
}

/// Portions d'un repas hors menu : les membres présents, avec leur barème.
pub async fn presence_scale(
    db: &PgPool,
    household_id: Uuid,
    date: Day,
    meal: Meal,
) -> ApiResult<(i32, Portions)> {
    let members = members::load(db, household_id, None).await?;
    Ok(part(&present(&members, date, meal), None))
}

// Écriture

fn in_span(week_start: Day, day: Day) -> ApiResult<()> {
    if day < week_start || day > week_start.plus(MENU_SPAN_DAYS) {
        return Err(ApiError::invalid(format!(
            "{day} : un menu va de son premier repas ({week_start}) à quatre semaines plus tard."
        )));
    }
    Ok(())
}

fn count(value: Option<i32>, what: &str) -> ApiResult<Option<i16>> {
    match value {
        None => Ok(None),
        Some(n) if (0..=99).contains(&n) => Ok(Some(n as i16)),
        Some(_) => Err(ApiError::invalid(format!(
            "{what} : un nombre entre 0 et 99."
        ))),
    }
}

/// Enregistre le menu (création ou remplacement complet). Renvoie `true` s'il a été créé.
pub(crate) async fn save(
    conn: &mut PgConnection,
    members: &[Member],
    household_id: Uuid,
    week_start: Day,
    input: &MenuInput,
) -> ApiResult<bool> {
    if input.week_start.is_some_and(|w| w != week_start) {
        return Err(ApiError::invalid(
            "La date du menu diffère de celle de l'adresse.",
        ));
    }
    if input.title.chars().count() > 120 || input.notes.chars().count() > 4000 {
        return Err(ApiError::invalid(
            "Titre de 120 caractères au plus, notes de 4 000.",
        ));
    }
    let slugs: Vec<&str> = input.slots.iter().map(|s| s.recipe.as_str()).collect();
    // Recettes du foyer et de la communauté (un slug n'est jamais aux deux).
    let recipes: HashMap<String, Uuid> = sqlx::query_as::<_, (String, Uuid)>(
        "SELECT slug, id FROM recipes WHERE (household_id = $1 OR household_id IS NULL) AND slug = ANY($2)",
    )
    .bind(household_id)
    .bind(&slugs)
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .collect();
    let ids: Vec<Uuid> = recipes.values().copied().collect();
    let labels: HashSet<(Uuid, String)> = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT recipe_id, label FROM recipe_variants WHERE recipe_id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .collect();

    struct Prepared {
        date: Day,
        meal: Meal,
        recipe_id: Uuid,
        variant: Option<String>,
        covers: bool,
        diners: Option<i16>,
        boxes: Option<i16>,
        portions_date: Option<Day>,
        note: Option<String>,
        thaw: Vec<String>,
    }
    let mut prepared = Vec::with_capacity(input.slots.len());
    for slot in &input.slots {
        let context = format!("{} {}", slot.date, slot.recipe);
        in_span(week_start, slot.date)?;
        let recipe_id = *recipes
            .get(&slot.recipe)
            .ok_or_else(|| ApiError::invalid(format!("{} : recette inconnue.", slot.recipe)))?;
        let variant = slot
            .variant
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty());
        if let Some(label) = variant
            && !labels.contains(&(recipe_id, label.to_owned()))
        {
            return Err(ApiError::invalid(format!(
                "{context} : variante inconnue « {label} »."
            )));
        }
        let (diners, boxes) = slot_counts(members, slot).map_err(|e| e.context(&context))?;
        if let Some(p) = slot.portions_date {
            in_span(week_start, p).map_err(|e| e.context(&context))?;
        }
        prepared.push(Prepared {
            date: slot.date,
            meal: slot.meal,
            recipe_id,
            variant: variant.map(str::to_owned),
            covers: slot.covers_next_lunch,
            diners,
            boxes: if slot.covers_next_lunch { boxes } else { None },
            portions_date: slot.portions_date,
            note: slot
                .note
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(str::to_owned),
            thaw: slot.thaw.clone(),
        });
    }

    let (menu_id, created): (Uuid, bool) = sqlx::query_as(
        "INSERT INTO menus (id, household_id, week_start, title, notes) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (household_id, week_start) DO UPDATE SET
            title = excluded.title, notes = excluded.notes, updated_at = now()
         RETURNING id, (xmax = 0)",
    )
    .bind(Uuid::now_v7())
    .bind(household_id)
    .bind(week_start.0)
    .bind(input.title.trim())
    .bind(input.notes.trim())
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM menu_slots WHERE menu_id = $1")
        .bind(menu_id)
        .execute(&mut *conn)
        .await?;
    let mut positions: HashMap<(Day, u8), i16> = HashMap::new();
    for slot in prepared {
        let position = positions
            .entry((slot.date, meal_order(slot.meal)))
            .or_default();
        sqlx::query(
            "INSERT INTO menu_slots (id, menu_id, date, meal, position, recipe_id, variant_label, covers_next_lunch,
                                     diners, next_lunch_covers, portions_date, note, thaw)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(Uuid::now_v7())
        .bind(menu_id)
        .bind(slot.date.0)
        .bind(slot.meal.as_str())
        .bind(*position)
        .bind(slot.recipe_id)
        .bind(&slot.variant)
        .bind(slot.covers)
        .bind(slot.diners)
        .bind(slot.boxes)
        .bind(slot.portions_date.map(|d| d.0))
        .bind(&slot.note)
        .bind(&slot.thaw)
        .execute(&mut *conn)
        .await?;
        *position += 1;
    }
    Ok(created)
}

/// Déplace un plat (voir [`MoveMode`]). Chaque plat qui change de jour garde les portions de son jour d'origine :
/// les courses sont faites.
fn plan_move(rows: &mut [SlotRow], week_start: Day, input: &MoveInput) -> ApiResult<()> {
    in_span(week_start, input.date)?;
    let moved = rows
        .iter()
        .position(|r| r.id == input.slot)
        .ok_or_else(ApiError::not_found)?;
    let target = (input.date, input.meal);
    let origin = (rows[moved].day(), rows[moved].meal);
    if origin == target {
        return Ok(());
    }
    let at = |rows: &[SlotRow], (day, meal): (Day, Meal)| -> Vec<usize> {
        let mut found: Vec<usize> = (0..rows.len())
            .filter(|&i| i != moved && rows[i].day() == day && rows[i].meal == meal)
            .collect();
        found.sort_by_key(|&i| rows[i].position);
        found
    };
    fn place(row: &mut SlotRow, (day, meal): (Day, Meal), position: i16) {
        if row.portions_date.is_none() {
            row.portions_date = Some(row.date);
        }
        row.date = day.0;
        row.meal = meal;
        row.position = position;
    }
    let occupants = at(rows, target);
    let siblings = at(rows, origin);
    // Le repas de départ se resserre.
    for (position, &i) in siblings.iter().enumerate() {
        rows[i].position = position as i16;
    }
    match input.mode {
        _ if occupants.is_empty() => place(&mut rows[moved], target, 0),
        MoveMode::Together => place(&mut rows[moved], target, occupants.len() as i16),
        MoveMode::Swap => {
            for (k, &i) in occupants.iter().enumerate() {
                place(&mut rows[i], origin, (siblings.len() + k) as i16);
            }
            place(&mut rows[moved], target, 0);
        }
        MoveMode::Insert => {
            place(&mut rows[moved], target, 0);
            let mut pushed = occupants;
            let mut day = target.0;
            while !pushed.is_empty() {
                day = day.plus(1);
                if day > week_start.plus(MENU_SPAN_DAYS) {
                    return Err(ApiError::invalid(
                        "Plus de repas libre dans le menu pour décaler les plats suivants.",
                    ));
                }
                let next = at(rows, (day, target.1));
                for (position, &i) in pushed.iter().enumerate() {
                    place(&mut rows[i], (day, target.1), position as i16);
                }
                pushed = next;
            }
        }
    }
    Ok(())
}

// Routes

/// Menus du foyer, du plus récent au plus ancien.
#[utoipa::path(
    get,
    path = "/api/v1/households/{id}/menus",
    tag = "menus",
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, body = Vec<MenuSummary>),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn list_menus(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<MenuSummary>>> {
    caller.require(&state.db, id, Scope::Read).await?;
    type Row = (Date, String, Option<Date>, Option<Date>, i64);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT m.week_start, m.title, min(s.date), max(s.date), count(s.id)
         FROM menus m LEFT JOIN menu_slots s ON s.menu_id = m.id
         WHERE m.household_id = $1 GROUP BY m.id ORDER BY m.week_start DESC",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(week_start, title, first, last, slot_count)| MenuSummary {
                week_start: Day(week_start),
                title,
                first_date: first.map(Day),
                last_date: last.map(Day),
                slot_count,
            })
            .collect(),
    ))
}

/// Menu complet : créneaux, portions, avertissements.
#[utoipa::path(
    get,
    path = "/api/v1/households/{id}/menus/{week_start}",
    tag = "menus",
    params(("id" = Uuid, Path), ("week_start" = String, Path, format = Date)),
    responses(
        (status = 200, body = Menu),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn read_menu(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, week_start)): Path<(Uuid, Day)>,
) -> ApiResult<Json<Menu>> {
    caller.require(&state.db, id, Scope::Read).await?;
    read_priced(&state.db, id, week_start, caller.user_id())
        .await
        .map(Json)
}

/// Publie un menu : le crée (201) ou remplace tous ses créneaux (200). Idempotent.
#[utoipa::path(
    put,
    path = "/api/v1/households/{id}/menus/{week_start}",
    tag = "menus",
    params(("id" = Uuid, Path), ("week_start" = String, Path, format = Date)),
    request_body = MenuInput,
    responses(
        (status = 200, description = "Remplacé", body = Menu),
        (status = 201, description = "Créé", body = Menu),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn put_menu(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, week_start)): Path<(Uuid, Day)>,
    Json(body): Json<MenuInput>,
) -> ApiResult<(StatusCode, Json<Menu>)> {
    caller.require(&state.db, id, Scope::MenusWrite).await?;
    let members = members::load(&state.db, id, None).await?;
    let mut tx = state.db.begin().await?;
    let created = save(&mut tx, &members, id, week_start, &body).await?;
    tx.commit().await?;
    state.notify(id, "menu", Some(week_start), None);
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((
        status,
        Json(read_priced(&state.db, id, week_start, caller.user_id()).await?),
    ))
}

#[utoipa::path(
    delete,
    path = "/api/v1/households/{id}/menus/{week_start}",
    tag = "menus",
    params(("id" = Uuid, Path), ("week_start" = String, Path, format = Date)),
    responses(
        (status = 204),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
    )
)]
pub async fn delete_menu(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, week_start)): Path<(Uuid, Day)>,
) -> ApiResult<StatusCode> {
    caller.require(&state.db, id, Scope::MenusWrite).await?;
    let deleted = sqlx::query("DELETE FROM menus WHERE household_id = $1 AND week_start = $2")
        .bind(id)
        .bind(week_start.0)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    state.notify(id, "menu", Some(week_start), None);
    Ok(StatusCode::NO_CONTENT)
}

/// Déplace un plat vers un autre repas du menu : repas libre, ou occupé (intercaler, échanger, même soir). Les
/// plats qui changent de jour gardent les portions de leur jour d'origine.
#[utoipa::path(
    post,
    path = "/api/v1/households/{id}/menus/{week_start}/move",
    tag = "menus",
    params(("id" = Uuid, Path), ("week_start" = String, Path, format = Date)),
    request_body = MoveInput,
    responses(
        (status = 200, body = Menu),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn move_slot(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, week_start)): Path<(Uuid, Day)>,
    Json(body): Json<MoveInput>,
) -> ApiResult<Json<Menu>> {
    caller.require(&state.db, id, Scope::MenusWrite).await?;
    let menu = menu_row(&state.db, id, week_start).await?;
    let mut tx = state.db.begin().await?;
    let mut rows: Vec<SlotRow> = sqlx::query_as(concat!(
        "SELECT ",
        slot_columns!(),
        " FROM menu_slots s WHERE s.menu_id = $1 FOR UPDATE"
    ))
    .bind(menu.id)
    .fetch_all(&mut *tx)
    .await?;
    let before: HashMap<Uuid, (Date, Meal, i16, Option<Date>)> = rows
        .iter()
        .map(|r| (r.id, (r.date, r.meal, r.position, r.portions_date)))
        .collect();
    plan_move(&mut rows, week_start, &body)?;
    for row in &rows {
        if before[&row.id] != (row.date, row.meal, row.position, row.portions_date) {
            sqlx::query("UPDATE menu_slots SET date = $2, meal = $3, position = $4, portions_date = $5 WHERE id = $1")
                .bind(row.id)
                .bind(row.date)
                .bind(row.meal.as_str())
                .bind(row.position)
                .bind(row.portions_date)
                .execute(&mut *tx)
                .await?;
        }
    }
    sqlx::query("UPDATE menus SET updated_at = now() WHERE id = $1")
        .bind(menu.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    state.notify(id, "menu", Some(week_start), None);
    read_priced(&state.db, id, week_start, caller.user_id())
        .await
        .map(Json)
}

/// Congeler un ingrédient d'un plat (en rentrant des courses) : l'alerte de fraîcheur tombe, et la veille du plat
/// rappelle de le sortir du congélateur. `frozen: false` annule.
#[utoipa::path(
    post,
    path = "/api/v1/households/{id}/menus/{week_start}/freeze",
    tag = "menus",
    params(("id" = Uuid, Path), ("week_start" = String, Path, format = Date)),
    request_body = FreezeInput,
    responses(
        (status = 200, body = Menu),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn freeze(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, week_start)): Path<(Uuid, Day)>,
    Json(body): Json<FreezeInput>,
) -> ApiResult<Json<Menu>> {
    caller.require(&state.db, id, Scope::MenusWrite).await?;
    let menu = menu_row(&state.db, id, week_start).await?;
    let in_recipe: Option<bool> = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM recipe_ingredients l JOIN ingredients i ON i.id = l.ingredient_id
                        WHERE l.recipe_id = s.recipe_id AND i.code = $3)
         FROM menu_slots s WHERE s.id = $1 AND s.menu_id = $2",
    )
    .bind(body.slot)
    .bind(menu.id)
    .bind(&body.ingredient)
    .fetch_optional(&state.db)
    .await?;
    match in_recipe {
        None => return Err(ApiError::not_found()),
        Some(false) => {
            return Err(ApiError::invalid(format!(
                "{} n'est pas dans ce plat.",
                body.ingredient
            )));
        }
        Some(true) => {}
    }
    sqlx::query(
        "UPDATE menu_slots SET thaw = CASE WHEN $3 THEN array_append(array_remove(thaw, $2), $2)
                                           ELSE array_remove(thaw, $2) END
         WHERE id = $1",
    )
    .bind(body.slot)
    .bind(&body.ingredient)
    .bind(body.frozen)
    .execute(&state.db)
    .await?;
    state.notify(id, "menu", Some(week_start), None);
    read_priced(&state.db, id, week_start, caller.user_id())
        .await
        .map(Json)
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AgendaQuery {
    /// Premier jour, `2026-10-18`.
    #[param(value_type = String, format = Date)]
    pub from: Day,
    /// Nombre de jours, de 1 à 28.
    pub days: u32,
}

/// Les repas d'une période, tous menus confondus, avec leurs recettes et les avertissements de la période.
#[utoipa::path(
    get,
    path = "/api/v1/households/{id}/agenda",
    tag = "menus",
    params(("id" = Uuid, Path), AgendaQuery),
    responses(
        (status = 200, body = Agenda),
        (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
pub async fn agenda(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Query(query): Query<AgendaQuery>,
) -> ApiResult<Json<Agenda>> {
    caller.require(&state.db, id, Scope::Read).await?;
    if !(1..=AGENDA_MAX_DAYS).contains(&query.days) {
        return Err(ApiError::invalid(format!(
            "De 1 à {AGENDA_MAX_DAYS} jours."
        )));
    }
    let from = query.from;
    let until = from.plus(i64::from(query.days) - 1);
    let menus: Vec<MenuRow> = sqlx::query_as(
        "SELECT m.id, m.week_start, m.title, m.notes FROM menus m
         WHERE m.household_id = $1
           AND EXISTS (SELECT 1 FROM menu_slots s WHERE s.menu_id = m.id AND s.date BETWEEN $2 AND $3)
         ORDER BY m.week_start",
    )
    .bind(id)
    .bind(from.0)
    .bind(until.0)
    .fetch_all(&state.db)
    .await?;
    let in_range = |day: Day| day >= from && day <= until;
    let mut slots = Vec::new();
    let mut warnings = Vec::new();
    for menu in menus {
        let computed = compute(&state.db, id, menu).await?;
        slots.extend(computed.slots.into_iter().filter(|s| in_range(s.date)));
        warnings.extend(computed.warnings.into_iter().filter(|w| in_range(w.date)));
    }
    slots.sort_by_key(|s| (s.date, meal_order(s.meal), s.week_start, s.position));
    nutrition::nutrition_slots(&state.db, id, caller.user_id(), &mut slots).await?;
    warnings.sort_by_key(|w| (w.date, meal_order(w.meal)));
    let mut slugs: Vec<&str> = slots.iter().map(|s| s.recipe.slug.as_str()).collect();
    slugs.sort_unstable();
    slugs.dedup();
    let mut recipes = Vec::with_capacity(slugs.len());
    for slug in slugs {
        recipes.push(recipes::load(&state.db, id, slug).await?);
    }
    Ok(Json(Agenda {
        from,
        days: query.days,
        slots,
        recipes,
        warnings,
    }))
}
