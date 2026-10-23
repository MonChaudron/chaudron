//! Prix estimés (design-prix.md) : chaque ingrédient, par forme d'achat, est rattaché à des sources publiques
//! (Open Prices, INSEE) ; une tâche mensuelle en tire un prix unitaire moyen, que les courses, le menu et les
//! recettes multiplient par leurs quantités. Pas de prix saisi à la main : seul un rattachement se corrige.

pub mod crossing;
pub mod insee;
pub mod open_prices;
pub mod run;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgExecutor, PgPool};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{error::ApiResult, menus::Day, types::IngredientState, types::text_enum};

/// Rattachements de départ, complétés au démarrage (jamais remplacés).
const SEED: &str = include_str!("../../data/prix.tsv");

text_enum! {
    /// Unité de référence d'un prix.
    pub enum PriceUnit {
        Kg = "kg",
        L = "l",
        Piece = "piece",
    }
}

text_enum! {
    /// D'où vient un prix.
    pub enum PriceSource {
        /// Médiane des relevés Open Prices récents.
        OpenPrices = "open_prices",
        /// Prix moyen de vente de détail de l'INSEE.
        Insee = "insee",
        /// Médiane Open Prices plus ancienne, actualisée par l'indice INSEE de son poste.
        Indexed = "indexed",
    }
}

/// Rattachement d'un ingrédient, pour une forme d'achat, aux sources de prix.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PriceLink {
    pub state: IngredientState,
    /// Catégorie Open Food Facts des relevés en vrac (`category_tag` d'Open Prices), ex. `en:carrots`.
    pub off_category: Option<String>,
    /// Catégorie Open Food Facts des produits à code-barres, ex. `en:gnocchi`.
    pub off_product_category: Option<String>,
    /// Idbank INSEE d'un prix moyen de vente de détail.
    pub insee_series: Option<String>,
    /// Unité de ce prix moyen (le kilo, sauf mention).
    pub insee_per: Option<PriceUnit>,
    /// Idbank INSEE de l'indice des prix du poste de l'ingrédient.
    pub insee_index: Option<String>,
}

/// Prix moyen estimé d'une unité de référence, pour une forme d'achat.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct UnitPrice {
    pub state: IngredientState,
    /// Centimes par unité de référence.
    pub cents: i32,
    pub per: PriceUnit,
    pub source: PriceSource,
    /// Relevés retenus (0 pour un prix INSEE).
    pub observations: i32,
    /// Date médiane des relevés retenus, ou mois du prix INSEE.
    #[schema(value_type = String, format = Date)]
    pub observed_on: Day,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub computed_at: OffsetDateTime,
}

#[derive(FromRow)]
struct LinkRow {
    ingredient_id: Uuid,
    #[sqlx(try_from = "String")]
    state: IngredientState,
    off_category: Option<String>,
    off_product_category: Option<String>,
    insee_series: Option<String>,
    insee_per: Option<String>,
    insee_index: Option<String>,
}

impl LinkRow {
    fn link(self) -> (Uuid, PriceLink) {
        (
            self.ingredient_id,
            PriceLink {
                state: self.state,
                off_category: self.off_category,
                off_product_category: self.off_product_category,
                insee_series: self.insee_series,
                insee_per: self.insee_per.and_then(|p| PriceUnit::try_from(p).ok()),
                insee_index: self.insee_index,
            },
        )
    }
}

#[derive(FromRow)]
struct PriceRow {
    ingredient_id: Uuid,
    #[sqlx(try_from = "String")]
    state: IngredientState,
    cents: i32,
    #[sqlx(try_from = "String")]
    per: PriceUnit,
    #[sqlx(try_from = "String")]
    source: PriceSource,
    observations: i32,
    observed_on: time::Date,
    computed_at: OffsetDateTime,
}

impl PriceRow {
    fn price(self) -> (Uuid, UnitPrice) {
        (
            self.ingredient_id,
            UnitPrice {
                state: self.state,
                cents: self.cents,
                per: self.per,
                source: self.source,
                observations: self.observations,
                observed_on: Day(self.observed_on),
                computed_at: self.computed_at,
            },
        )
    }
}

/// Rattachements (de tous les ingrédients, ou d'un seul), dans l'ordre des formes d'achat.
pub async fn links(
    db: impl PgExecutor<'_>,
    only: Option<Uuid>,
) -> ApiResult<Vec<(Uuid, PriceLink)>> {
    let rows: Vec<LinkRow> = sqlx::query_as(
        "SELECT ingredient_id, state, off_category, off_product_category, insee_series, insee_per, insee_index
         FROM ingredient_price_links WHERE $1::uuid IS NULL OR ingredient_id = $1 ORDER BY array_position(ARRAY['fresh', 'frozen', 'canned', 'dried', 'cooked'], state)",
    )
    .bind(only)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(LinkRow::link).collect())
}

/// Prix unitaires (de tous les ingrédients, ou d'un seul), dans l'ordre des formes d'achat.
pub async fn unit_prices(
    db: impl PgExecutor<'_>,
    only: Option<Uuid>,
) -> ApiResult<Vec<(Uuid, UnitPrice)>> {
    let rows: Vec<PriceRow> = sqlx::query_as(
        "SELECT ingredient_id, state, cents, per, source, observations, observed_on, computed_at
         FROM ingredient_prices WHERE $1::uuid IS NULL OR ingredient_id = $1 ORDER BY array_position(ARRAY['fresh', 'frozen', 'canned', 'dried', 'cooked'], state)",
    )
    .bind(only)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(PriceRow::price).collect())
}

/// Ce qu'il faut pour chiffrer des lignes : prix unitaires, poids des pièces, densités.
#[derive(Default)]
pub struct PriceBook {
    pub prices: HashMap<(Uuid, IngredientState), UnitPrice>,
    pub piece_g: HashMap<Uuid, f32>,
    pub density: HashMap<Uuid, f32>,
}

impl PriceBook {
    pub async fn load(db: &PgPool, ids: &[Uuid]) -> ApiResult<Self> {
        let rows: Vec<PriceRow> = sqlx::query_as(
            "SELECT ingredient_id, state, cents, per, source, observations, observed_on, computed_at
             FROM ingredient_prices WHERE ingredient_id = ANY($1)",
        )
        .bind(ids)
        .fetch_all(db)
        .await?;
        let sizes: Vec<(Uuid, Option<f32>, Option<f32>)> =
            sqlx::query_as("SELECT id, piece_g, density FROM ingredients WHERE id = ANY($1)")
                .bind(ids)
                .fetch_all(db)
                .await?;
        let mut book = Self::default();
        for (id, price) in rows.into_iter().map(PriceRow::price) {
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
        Ok(book)
    }

    /// Le jour du plus ancien calcul parmi les prix du carnet : « mis à jour le … ».
    pub fn updated_on(&self) -> Option<Day> {
        self.prices
            .values()
            .map(|p| Day(p.computed_at.date()))
            .min()
    }
}

/// Une ligne de l'amorçage.
struct SeedLine {
    code: String,
    link: PriceLink,
    piece_g: Option<f32>,
    density: Option<f32>,
}

fn field(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn parse_seed() -> Vec<SeedLine> {
    SEED.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            assert_eq!(f.len(), 9, "ligne de prix.tsv : {line}");
            SeedLine {
                code: f[0].to_owned(),
                link: PriceLink {
                    state: IngredientState::try_from(f[1].to_owned())
                        .expect("forme d'achat de prix.tsv"),
                    off_category: field(f[2]),
                    off_product_category: field(f[3]),
                    insee_series: field(f[4]),
                    insee_per: field(f[5])
                        .map(|p| PriceUnit::try_from(p).expect("unité de prix.tsv")),
                    insee_index: field(f[6]),
                },
                piece_g: field(f[7]).map(|v| v.parse().expect("poids de pièce de prix.tsv")),
                density: field(f[8]).map(|v| v.parse().expect("densité de prix.tsv")),
            }
        })
        .collect()
}

/// Complète, pour les ingrédients qui existent, les rattachements absents et les poids et densités inconnus.
/// Ne remplace rien. Renvoie le nombre de rattachements et d'ingrédients complétés.
pub async fn seed_links(db: &PgPool) -> sqlx::Result<u64> {
    let mut tx = db.begin().await?;
    let mut added = 0;
    for line in parse_seed() {
        let l = &line.link;
        added += sqlx::query(
            "INSERT INTO ingredient_price_links
                 (ingredient_id, state, off_category, off_product_category, insee_series, insee_per, insee_index)
             SELECT id, $2, $3, $4, $5, $6, $7 FROM ingredients WHERE code = $1
             ON CONFLICT (ingredient_id, state) DO NOTHING",
        )
        .bind(&line.code)
        .bind(l.state.as_str())
        .bind(&l.off_category)
        .bind(&l.off_product_category)
        .bind(&l.insee_series)
        .bind(l.insee_per.map(PriceUnit::as_str))
        .bind(&l.insee_index)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if line.piece_g.is_some() || line.density.is_some() {
            added += sqlx::query(
                "UPDATE ingredients SET piece_g = coalesce(piece_g, $2), density = coalesce(density, $3)
                 WHERE code = $1 AND ((piece_g IS NULL AND $2 IS NOT NULL) OR (density IS NULL AND $3 IS NOT NULL))",
            )
            .bind(&line.code)
            .bind(line.piece_g)
            .bind(line.density)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        }
    }
    tx.commit().await?;
    Ok(added)
}
