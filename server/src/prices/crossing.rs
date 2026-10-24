//! Le croisement des sources, ingrédient par ingrédient, et le passage des unités de recette aux unités de prix.
//! Sans accès réseau ni base : tout se teste ici.

use crate::{
    menus::Day,
    types::Unit,
    units::{PINCH_G, TBSP_ML, TSP_ML},
};

use super::{PriceSource, PriceUnit, UnitPrice};

/// Labels dont les relevés sont écartés : la gamme retenue est le conventionnel, toutes marques.
const EXCLUDED_LABELS: &[&str] = &[
    "en:organic",
    "en:eu-organic",
    "fr:ab-agriculture-biologique",
    "fr:label-rouge",
];

/// Écart Open Prices / INSEE au-delà duquel le rattachement est signalé (rapporté au prix INSEE).
const DIVERGENCE: f64 = 0.5;

/// Un relevé de prix, déjà ramené à l'unité de référence de l'ingrédient.
#[derive(Clone, Debug)]
pub struct Observation {
    pub cents: f64,
    pub per: PriceUnit,
    pub date: Day,
    pub labels: Vec<String>,
}

/// Un point d'indice des prix (mois, valeur).
#[derive(Clone, Debug)]
pub struct IndexPoint {
    pub month: Day,
    pub value: f64,
}

/// Le prix retenu pour un ingrédient.
#[derive(Clone, Debug, PartialEq)]
pub struct Crossed {
    pub cents: i32,
    pub source: PriceSource,
    pub observations: i32,
    pub observed_on: Day,
    pub insee_cents: Option<i32>,
    pub divergent: bool,
}

/// Médiane des relevés depuis `since`, labels écartés et valeurs aberrantes retirées (hors de
/// [Q1 − 1,5 × IQR, Q3 + 1,5 × IQR]) ; `None` s'il en reste moins de `min`. Renvoie la médiane, la date médiane des
/// relevés retenus et leur nombre.
pub fn robust_median(obs: &[Observation], since: Day, min: usize) -> Option<(f64, Day, usize)> {
    let mut kept: Vec<&Observation> = obs
        .iter()
        .filter(|o| o.date >= since && o.cents > 0.0)
        .filter(|o| {
            !o.labels
                .iter()
                .any(|l| EXCLUDED_LABELS.contains(&l.as_str()))
        })
        .collect();
    let mut values: Vec<f64> = kept.iter().map(|o| o.cents).collect();
    values.sort_by(f64::total_cmp);
    if values.len() >= 4 {
        let (q1, q3) = (quantile(&values, 0.25), quantile(&values, 0.75));
        let spread = 1.5 * (q3 - q1);
        kept.retain(|o| o.cents >= q1 - spread && o.cents <= q3 + spread);
    }
    if kept.len() < min.max(1) {
        return None;
    }
    let mut values: Vec<f64> = kept.iter().map(|o| o.cents).collect();
    values.sort_by(f64::total_cmp);
    kept.sort_by_key(|o| o.date);
    Some((
        quantile(&values, 0.5),
        kept[(kept.len() - 1) / 2].date,
        kept.len(),
    ))
}

/// Quantile d'une liste triée, par interpolation linéaire.
fn quantile(sorted: &[f64], p: f64) -> f64 {
    let at = p * (sorted.len() - 1) as f64;
    let (low, high) = (at.floor() as usize, at.ceil() as usize);
    sorted[low] + (sorted[high] - sorted[low]) * (at - low as f64)
}

/// Les relevés au poids ou au volume quand il y en a assez : un relevé « à l'unité » peut être un filet ou une
/// barquette, le poids ne se trompe pas. Sinon, tous.
pub fn prefer_weighed(obs: Vec<Observation>, min: usize) -> Vec<Observation> {
    let weighed = obs.iter().filter(|o| o.per != PriceUnit::Piece).count();
    if weighed < min {
        return obs;
    }
    obs.into_iter()
        .filter(|o| o.per != PriceUnit::Piece)
        .collect()
}

/// Croise une médiane récente, une médiane plus ancienne, le prix moyen INSEE et l'indice du poste (design-prix.md,
/// « Le croisement »).
pub fn cross(
    recent: Option<(f64, Day, usize)>,
    older: Option<(f64, Day, usize)>,
    insee: Option<(f64, Day)>,
    index: &[IndexPoint],
) -> Option<Crossed> {
    let insee_cents = insee.map(|(c, _)| c.round() as i32);
    if let Some((cents, on, n)) = recent {
        let divergent = insee.is_some_and(|(i, _)| i > 0.0 && ((cents - i) / i).abs() > DIVERGENCE);
        return Some(Crossed {
            cents: cents.round() as i32,
            source: PriceSource::OpenPrices,
            observations: n as i32,
            observed_on: on,
            insee_cents,
            divergent,
        });
    }
    if let Some((cents, month)) = insee {
        return Some(Crossed {
            cents: cents.round() as i32,
            source: PriceSource::Insee,
            observations: 0,
            observed_on: month,
            insee_cents,
            divergent: false,
        });
    }
    let (cents, on, n) = older?;
    let same_month =
        |p: &&IndexPoint| p.month.0.year() == on.0.year() && p.month.0.month() == on.0.month();
    let then = index.iter().find(same_month)?;
    let latest = index.iter().max_by_key(|p| p.month)?;
    (then.value > 0.0).then(|| Crossed {
        cents: (cents * latest.value / then.value).round() as i32,
        source: PriceSource::Indexed,
        observations: n as i32,
        observed_on: on,
        insee_cents: None,
        divergent: false,
    })
}

/// Convertit un prix d'une unité à une autre ; `None` s'il manque le poids d'une pièce ou la densité.
pub fn convert(
    cents: f64,
    from: PriceUnit,
    to: PriceUnit,
    piece_g: Option<f32>,
    density: Option<f32>,
) -> Option<f64> {
    // Grammes d'une unité de chaque sorte.
    let grams = |unit: PriceUnit| -> Option<f64> {
        match unit {
            PriceUnit::Kg => Some(1000.0),
            PriceUnit::L => density.map(|d| 1000.0 * f64::from(d)),
            PriceUnit::Piece => piece_g.map(f64::from),
        }
    };
    if from == to {
        return Some(cents);
    }
    Some(cents / grams(from)? * grams(to)?)
}

/// Prix d'une quantité de recette ou de courses ; `None` si l'unité ne se convertit pas.
pub fn line_cents(
    qty: f32,
    unit: Unit,
    price: &UnitPrice,
    piece_g: Option<f32>,
    density: Option<f32>,
) -> Option<f64> {
    let qty = f64::from(qty);
    // La quantité dans une unité de prix : kilos, litres ou pièces.
    let (amount, per) = match unit {
        Unit::G => (qty / 1000.0, PriceUnit::Kg),
        Unit::Pinch => (qty * PINCH_G / 1000.0, PriceUnit::Kg),
        Unit::Ml => (qty / 1000.0, PriceUnit::L),
        Unit::Tbsp => (qty * TBSP_ML / 1000.0, PriceUnit::L),
        Unit::Tsp => (qty * TSP_ML / 1000.0, PriceUnit::L),
        Unit::Piece => (qty, PriceUnit::Piece),
    };
    Some(amount * convert(f64::from(price.cents), price.per, per, piece_g, density)?)
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use super::*;
    use crate::types::IngredientState;

    fn day(text: &str) -> Day {
        Day::parse(text).unwrap()
    }

    fn obs(cents: f64, date: &str, labels: &[&str]) -> Observation {
        Observation {
            cents,
            per: PriceUnit::Kg,
            date: day(date),
            labels: labels.iter().map(|l| (*l).to_owned()).collect(),
        }
    }

    fn price(cents: i32, per: PriceUnit) -> UnitPrice {
        UnitPrice {
            state: IngredientState::Fresh,
            cents,
            per,
            source: PriceSource::OpenPrices,
            observations: 5,
            observed_on: day("2026-10-01"),
            computed_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn mediane_ecarte_le_bio_et_l_aberrant() {
        let relevés = [
            obs(229.0, "2026-10-08", &[]),
            obs(245.0, "2026-10-10", &["en:nutriscore"]),
            obs(262.0, "2026-10-12", &[]),
            obs(280.0, "2026-10-14", &[]),
            obs(284.0, "2026-10-16", &[]),
            obs(1270.0, "2026-09-01", &[]),
            obs(310.0, "2026-10-01", &["en:organic"]),
        ];
        let (median, on, n) = robust_median(&relevés, day("2026-04-23"), 5).unwrap();
        assert_eq!(median, 262.0);
        assert_eq!(n, 5);
        assert_eq!(on, day("2026-10-12"));
    }

    #[test]
    fn trop_peu_de_releves() {
        let relevés = [
            obs(229.0, "2026-10-08", &[]),
            obs(245.0, "2026-10-10", &[]),
            obs(262.0, "2026-10-12", &[]),
            obs(280.0, "2026-10-14", &[]),
            obs(300.0, "2026-01-14", &[]),
        ];
        assert_eq!(
            robust_median(&relevés, day("2026-04-23"), 5),
            None,
            "le relevé de janvier est trop ancien"
        );
    }

    #[test]
    fn insee_en_secours() {
        let crossed = cross(None, None, Some((255.0, day("2026-09-01"))), &[]).unwrap();
        assert_eq!(crossed.cents, 255);
        assert_eq!(crossed.source, PriceSource::Insee);
        assert_eq!(crossed.observed_on, day("2026-09-01"));
    }

    #[test]
    fn ancienne_mediane_actualisee() {
        let index = [
            IndexPoint {
                month: day("2026-01-01"),
                value: 100.0,
            },
            IndexPoint {
                month: day("2026-05-01"),
                value: 104.0,
            },
            IndexPoint {
                month: day("2026-09-01"),
                value: 110.0,
            },
        ];
        let crossed = cross(None, Some((200.0, day("2026-01-15"), 7)), None, &index).unwrap();
        assert_eq!(crossed.cents, 220);
        assert_eq!(crossed.source, PriceSource::Indexed);
        assert_eq!(crossed.observations, 7);
    }

    #[test]
    fn ancienne_mediane_sans_indice_inutilisable() {
        assert_eq!(
            cross(None, Some((200.0, day("2026-01-15"), 7)), None, &[]),
            None
        );
    }

    #[test]
    fn ecart_signale() {
        let recent = Some((400.0, day("2026-10-01"), 5));
        let insee = Some((250.0, day("2026-09-01")));
        let crossed = cross(recent, None, insee, &[]).unwrap();
        assert_eq!(crossed.source, PriceSource::OpenPrices);
        assert_eq!(crossed.cents, 400);
        assert_eq!(crossed.insee_cents, Some(250));
        assert!(crossed.divergent);

        let close = cross(Some((300.0, day("2026-10-01"), 5)), None, insee, &[]).unwrap();
        assert!(!close.divergent);
    }

    #[test]
    fn ligne_non_convertible_sans_prix() {
        let garlic = price(1200, PriceUnit::Kg);
        assert_eq!(line_cents(2.0, Unit::Piece, &garlic, None, None), None);
        assert_eq!(
            line_cents(2.0, Unit::Piece, &garlic, Some(5.0), None),
            Some(12.0)
        );
    }

    #[test]
    fn cuilleres_et_densite() {
        let soy = price(600, PriceUnit::L);
        assert!((line_cents(3.0, Unit::Tbsp, &soy, None, None).unwrap() - 27.0).abs() < 1e-9);
        let oil = price(900, PriceUnit::L);
        assert!((line_cents(1.0, Unit::Tbsp, &oil, None, Some(0.92)).unwrap() - 13.5).abs() < 1e-9);
        assert_eq!(
            line_cents(100.0, Unit::G, &oil, None, None),
            None,
            "des grammes d'huile sans densité"
        );
    }

    #[test]
    fn releves_au_poids_preferes_a_l_unite() {
        let mut relevés: Vec<Observation> = (0..5)
            .map(|d| obs(280.0, &format!("2026-10-0{}", d + 1), &[]))
            .collect();
        let bag = |date: &str| Observation {
            cents: 276.0,
            per: PriceUnit::Piece,
            date: day(date),
            labels: Vec::new(),
        };
        relevés.extend(
            [
                "2026-10-06",
                "2026-10-07",
                "2026-10-08",
                "2026-10-09",
                "2026-10-10",
                "2026-10-11",
            ]
            .map(bag),
        );
        let kept = prefer_weighed(relevés.clone(), 5);
        assert_eq!(
            kept.len(),
            5,
            "un filet d'oignons « à l'unité » n'est pas un oignon"
        );
        assert!(kept.iter().all(|o| o.per == PriceUnit::Kg));

        let few: Vec<Observation> = relevés.into_iter().skip(3).collect();
        assert_eq!(
            prefer_weighed(few, 5).len(),
            8,
            "trop peu de relevés au poids : on garde tout"
        );
    }

    #[test]
    fn pieces_et_kilos() {
        let near = |got: Option<f64>, want: f64| got.is_some_and(|g| (g - want).abs() < 1e-3);
        assert!(near(
            convert(360.0, PriceUnit::Piece, PriceUnit::Kg, Some(800.0), None),
            450.0
        ));
        assert!(near(
            convert(300.0, PriceUnit::Kg, PriceUnit::Piece, Some(200.0), None),
            60.0
        ));
        assert!(near(
            convert(600.0, PriceUnit::L, PriceUnit::Kg, None, Some(1.2)),
            500.0
        ));
        assert_eq!(
            convert(600.0, PriceUnit::L, PriceUnit::Kg, None, None),
            None
        );
    }
}
