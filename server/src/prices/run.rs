//! La mise à jour des prix : un passage complet quand le dernier réussi a plus de 30 jours, et le chiffrage
//! d'ingrédients précis (nouvellement rattachés).

use std::time::Duration;

use sqlx::FromRow;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    AppState,
    menus::Day,
    types::{IngredientState, Unit},
};

use super::{
    PriceLink, PriceUnit,
    crossing::{Crossed, Observation, convert, cross, robust_median},
    insee, open_prices,
};

/// Relevés récents (6 mois), relevés anciens pour l'actualisation (24 mois), relevés retenus au moins.
const RECENT_DAYS: i64 = 183;
const OLDER_DAYS: i64 = 730;
const MIN_OBSERVATIONS: usize = 5;
/// Un passage complet par mois ; contrôlé au démarrage puis toutes les 6 h.
const DUE_DAYS: i32 = 30;
const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);

/// Un ingrédient, dans une forme d'achat, à chiffrer.
#[derive(FromRow)]
pub(crate) struct Target {
    pub ingredient_id: Uuid,
    pub code: String,
    #[sqlx(try_from = "String")]
    pub state: IngredientState,
    #[sqlx(try_from = "String")]
    pub default_unit: Unit,
    pub piece_g: Option<f32>,
    pub density: Option<f32>,
    pub off_category: Option<String>,
    pub off_product_category: Option<String>,
    pub insee_series: Option<String>,
    pub insee_per: Option<String>,
    pub insee_index: Option<String>,
}

impl Target {
    pub(crate) fn new(
        ingredient_id: Uuid,
        code: String,
        default_unit: Unit,
        piece_g: Option<f32>,
        density: Option<f32>,
        link: &PriceLink,
    ) -> Self {
        Self {
            ingredient_id,
            code,
            state: link.state,
            default_unit,
            piece_g,
            density,
            off_category: link.off_category.clone(),
            off_product_category: link.off_product_category.clone(),
            insee_series: link.insee_series.clone(),
            insee_per: link.insee_per.map(|p| p.as_str().to_owned()),
            insee_index: link.insee_index.clone(),
        }
    }

    fn link(&self) -> PriceLink {
        PriceLink {
            state: self.state,
            off_category: self.off_category.clone(),
            off_product_category: self.off_product_category.clone(),
            insee_series: self.insee_series.clone(),
            insee_per: self
                .insee_per
                .clone()
                .and_then(|p| PriceUnit::try_from(p).ok()),
            insee_index: self.insee_index.clone(),
        }
    }

    /// L'unité de référence du prix, d'après l'unité de l'ingrédient dans les recettes.
    pub(crate) fn reference(&self) -> PriceUnit {
        match self.default_unit {
            Unit::G | Unit::Pinch => PriceUnit::Kg,
            Unit::Ml | Unit::Tbsp | Unit::Tsp => PriceUnit::L,
            Unit::Piece => PriceUnit::Piece,
        }
    }

    fn to_reference(&self, cents: f64, from: PriceUnit) -> Option<f64> {
        convert(cents, from, self.reference(), self.piece_g, self.density)
    }
}

/// Chiffre les ingrédients rattachés (tous, ou ceux-là) ; renvoie le nombre de prix écrits. Une source en échec
/// laisse les prix précédents et rend une erreur (les prix des autres ingrédients sont écrits quand même).
pub async fn price_ingredients(state: &AppState, ids: Option<&[Uuid]>) -> Result<u32, String> {
    let targets: Vec<Target> = sqlx::query_as(
        "SELECT l.ingredient_id, i.code, l.state, i.default_unit, i.piece_g, i.density, l.off_category,
                l.off_product_category, l.insee_series, l.insee_per, l.insee_index
         FROM ingredient_price_links l JOIN ingredients i ON i.id = l.ingredient_id
         WHERE $1::uuid[] IS NULL OR l.ingredient_id = ANY($1) ORDER BY i.code, l.state",
    )
    .bind(ids)
    .fetch_all(&state.db)
    .await
    .map_err(|err| err.to_string())?;
    let today = Day(OffsetDateTime::now_utc().date());
    let mut priced = 0;
    let mut errors = Vec::new();
    for target in &targets {
        match price_one(state, target, today).await {
            Ok(Some(crossed)) => {
                store(state, target, &crossed)
                    .await
                    .map_err(|err| err.to_string())?;
                priced += 1;
            }
            Ok(None) => {}
            Err(err) => errors.push(format!("{} : {err}", target.code)),
        }
    }
    if errors.is_empty() {
        Ok(priced)
    } else {
        Err(format!(
            "{priced} prix écrits ; {} en échec : {}",
            errors.len(),
            errors.join(" ; ")
        ))
    }
}

/// Le croisement pour un ingrédient (design-prix.md, « Le croisement »).
pub(crate) async fn price_one(
    state: &AppState,
    target: &Target,
    today: Day,
) -> Result<Option<Crossed>, String> {
    let sources = &state.config.prices;
    let link = target.link();
    let observed = if link.off_category.is_some() || link.off_product_category.is_some() {
        open_prices::observations(
            &state.http,
            &sources.open_prices_url,
            &link,
            today.plus(-OLDER_DAYS),
            sources.pause,
        )
        .await?
    } else {
        Vec::new()
    };
    let observed: Vec<Observation> = observed
        .into_iter()
        .filter_map(|o| {
            Some(Observation {
                cents: target.to_reference(o.cents, o.per)?,
                per: target.reference(),
                ..o
            })
        })
        .collect();
    let recent = robust_median(&observed, today.plus(-RECENT_DAYS), MIN_OBSERVATIONS);
    let older = match recent {
        Some(_) => None,
        None => robust_median(&observed, today.plus(-OLDER_DAYS), MIN_OBSERVATIONS),
    };
    let insee = match &link.insee_series {
        Some(idbank) => {
            let points = insee::series(&state.http, &sources.insee_url, idbank, 1).await?;
            tokio::time::sleep(sources.pause).await;
            points.first().and_then(|p| {
                let per = link.insee_per.unwrap_or(PriceUnit::Kg);
                Some((target.to_reference(p.value * 100.0, per)?, p.month))
            })
        }
        None => None,
    };
    let index = match (&link.insee_index, recent, insee, older) {
        (Some(idbank), None, None, Some(_)) => {
            let points = insee::series(&state.http, &sources.insee_url, idbank, 24).await?;
            tokio::time::sleep(sources.pause).await;
            points
        }
        _ => Vec::new(),
    };
    Ok(cross(recent, older, insee, &index))
}

async fn store(state: &AppState, target: &Target, crossed: &Crossed) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO ingredient_prices
             (ingredient_id, state, cents, per, source, observations, observed_on, insee_cents, flag, computed_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now())
         ON CONFLICT (ingredient_id, state) DO UPDATE SET
             cents = excluded.cents, per = excluded.per, source = excluded.source,
             observations = excluded.observations, observed_on = excluded.observed_on,
             insee_cents = excluded.insee_cents, flag = excluded.flag, computed_at = excluded.computed_at",
    )
    .bind(target.ingredient_id)
    .bind(target.state.as_str())
    .bind(crossed.cents.max(1))
    .bind(target.reference().as_str())
    .bind(crossed.source.as_str())
    .bind(crossed.observations)
    .bind(crossed.observed_on.0)
    .bind(crossed.insee_cents)
    .bind(crossed.divergent.then_some("divergent"))
    .execute(&state.db)
    .await?;
    Ok(())
}

/// Lance un passage complet s'il est dû ; renvoie `true` s'il a eu lieu (réussi ou non).
pub async fn check(state: &AppState) -> bool {
    match run_if_due(state).await {
        Ok(ran) => ran,
        Err(err) => {
            tracing::warn!(%err, "mise à jour des prix");
            false
        }
    }
}

async fn run_if_due(state: &AppState) -> sqlx::Result<bool> {
    let busy: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM price_runs
                        WHERE (ok AND started_at > now() - make_interval(days => $1))
                           OR (finished_at IS NULL AND started_at > now() - interval '6 hours'))",
    )
    .bind(DUE_DAYS)
    .fetch_one(&state.db)
    .await?;
    if busy {
        return Ok(false);
    }
    run(state).await?;
    Ok(true)
}

/// Un passage complet, consigné dans `price_runs`.
pub async fn run(state: &AppState) -> sqlx::Result<()> {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO price_runs (id) VALUES ($1)")
        .bind(id)
        .execute(&state.db)
        .await?;
    let outcome = price_ingredients(state, None).await;
    let (ok, priced, message) = match &outcome {
        Ok(priced) => (true, *priced as i32, String::new()),
        Err(err) => (false, 0, err.clone()),
    };
    match &outcome {
        Ok(priced) => tracing::info!(priced, "prix mis à jour"),
        Err(err) => tracing::warn!(%err, "mise à jour des prix incomplète"),
    }
    sqlx::query("UPDATE price_runs SET finished_at = now(), ok = $2, priced = $3, message = $4 WHERE id = $1")
        .bind(id)
        .bind(ok)
        .bind(priced)
        .bind(message)
        .execute(&state.db)
        .await?;
    Ok(())
}

/// La tâche de fond : un contrôle au démarrage, puis toutes les 6 h.
pub fn schedule(state: AppState) {
    tokio::spawn(async move {
        loop {
            check(&state).await;
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}
