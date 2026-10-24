//! Les apports d'une portion, ses poids cuits et sa note, à partir des lignes d'une recette et de leurs teneurs
//! Ciqual. Sans base : tout se teste ici.

use crate::{
    members::Portions,
    prices::estimate::BATCH_PORTIONS,
    types::{IngredientState, PortionRole, Unit},
    units::grams,
};

use super::{
    Amounts, Criterion, DailyPct, EnergySplit, Factor, Nutrition, PortionKind, Serve,
    score::{Per100g, points},
};

/// Apports de référence d'un adulte (règlement INCO, annexe XIII ; fibres : Anses), dans l'ordre des teneurs.
const REFERENCE: [f64; 8] = [2000.0, 50.0, 260.0, 90.0, 70.0, 20.0, 30.0, 6.0];
const KJ_PER_KCAL: f64 = 4.184;
/// Indices des teneurs : énergie kcal, protéines, glucides, sucres, lipides, AG saturés, fibres, sel.
const ENERGY: usize = 0;
const PROTEIN: usize = 1;
const CARBS: usize = 2;
const SUGARS: usize = 3;
const FAT: usize = 4;
const SAT_FAT: usize = 5;
const FIBER: usize = 6;
const SALT: usize = 7;
/// Sous-groupes Ciqual des fruits, légumes et légumineuses de l'algorithme : ni pommes de terre (0202), ni fruits à
/// coque (0205), ni huiles.
const FVL_SUBGROUPS: &[&str] = &["0201", "0203", "0204"];
/// Viandes cuites, crues, charcuteries, autres produits carnés.
const MEAT_SUBGROUPS: &[&str] = &["0401", "0402", "0403", "0404"];
/// Ce qui, dans ces sous-groupes, n'est pas de la viande rouge.
const NOT_RED_MEAT: &[&str] = &[
    "poulet", "dinde", "canard", "pintade", "oie", "caille", "lapin", "volaille", "poule",
    "chapon", "pigeon", "autruche", "faisan", "perdrix",
];

/// La ligne Ciqual d'un ingrédient : sous-groupe, nom normalisé, teneurs pour 100 g (vide : non renseignée).
#[derive(Clone, Debug)]
pub struct Food {
    pub subgroup: String,
    pub name_key: String,
    pub amounts: [Option<f32>; 8],
}

/// Une ligne de recette, pour une portion standard, avec ce qu'il faut pour la peser.
#[derive(Clone, Debug)]
pub struct Line {
    pub name: String,
    pub role: PortionRole,
    pub state: IngredientState,
    pub qty: Option<f32>,
    pub unit: Unit,
    pub optional: bool,
    pub per_batch: bool,
    pub piece_g: Option<f32>,
    pub density: Option<f32>,
    pub cooked_ratio: Option<f32>,
    pub food: Option<Food>,
}

/// Rendement de cuisson (cuit / cru) quand l'ingrédient n'en a pas : les féculents secs prennent l'eau, la viande et
/// le poisson en perdent, les légumes un peu.
pub fn default_cooked_ratio(subgroup: &str, state: IngredientState) -> f32 {
    use IngredientState::*;
    match (subgroup, state) {
        (_, Cooked) => 1.0,
        ("0301" | "0203", Dried) => 2.5,
        ("0402", _) => 0.75,
        ("0406" | "0408", _) => 0.8,
        ("0403", _) => 0.9,
        ("0201", Fresh | Frozen) => 0.9,
        _ => 1.0,
    }
}

/// Bœuf, veau, porc, agneau, gibier et leurs produits : le plafond des protéines de l'algorithme.
pub fn is_red_meat(subgroup: &str, name_key: &str) -> bool {
    MEAT_SUBGROUPS.contains(&subgroup) && !NOT_RED_MEAT.iter().any(|w| name_key.contains(w))
}

/// Une ligne pesée : grammes crus pour la portion, et cuits.
struct Weighed<'a> {
    line: &'a Line,
    food: &'a Food,
    raw_g: f64,
    cooked_g: f64,
}

impl Weighed<'_> {
    fn amount(&self, index: usize) -> f64 {
        self.raw_g * f64::from(self.food.amounts[index].unwrap_or(0.0)) / 100.0
    }

    fn is_fvl(&self) -> bool {
        FVL_SUBGROUPS.contains(&self.food.subgroup.as_str())
    }
}

/// Les lignes pesées, les ingrédients sans données, et si l'un d'eux fausse la note (une épice, non).
fn weigh<'a>(
    lines: &'a [Line],
    portions: Option<&Portions>,
) -> (Vec<Weighed<'a>>, Vec<String>, bool) {
    let mut weighed = Vec::new();
    let mut missing = Vec::new();
    let mut blocks = false;
    for line in lines.iter().filter(|l| !l.optional) {
        let Some(qty) = line.qty else { continue };
        let g = grams(qty, line.unit, line.piece_g, line.density);
        let (Some(food), Some(g)) = (&line.food, g) else {
            missing.push(line.name.clone());
            blocks |= !matches!(line.unit, Unit::Pinch | Unit::Tsp);
            continue;
        };
        let factor = if line.per_batch {
            1.0 / BATCH_PORTIONS
        } else {
            portions.map_or(1.0, |p| f64::from(p.for_role(line.role)))
        };
        let raw_g = g * factor;
        let ratio = line
            .cooked_ratio
            .unwrap_or_else(|| default_cooked_ratio(&food.subgroup, line.state));
        weighed.push(Weighed {
            line,
            food,
            raw_g,
            cooked_g: raw_g * f64::from(ratio),
        });
    }
    (weighed, missing, blocks)
}

fn totals(weighed: &[Weighed]) -> [f64; 8] {
    std::array::from_fn(|i| weighed.iter().map(|w| w.amount(i)).sum())
}

/// Un ingrédient et ce qu'il apporte.
type Top = (String, f32);

/// La ligne qui apporte le plus d'une teneur, et combien.
fn top(weighed: &[Weighed], value: impl Fn(&Weighed) -> f64) -> Option<Top> {
    weighed
        .iter()
        .map(|w| (w, value(w)))
        .filter(|(_, v)| *v > 0.0)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(w, v)| (w.line.name.clone(), v as f32))
}

fn factors(candidates: Vec<(Criterion, i32, Option<Top>)>) -> Vec<Factor> {
    let mut kept: Vec<_> = candidates.into_iter().filter(|c| c.1 > 0).collect();
    kept.sort_by_key(|c| std::cmp::Reverse(c.1));
    kept.into_iter()
        .take(2)
        .map(|(criterion, points, top)| Factor {
            criterion,
            points,
            ingredient: top.as_ref().map(|t| t.0.clone()),
            amount: top.map(|t| t.1),
        })
        .collect()
}

/// D'où viennent les calories, en % entiers dont la somme fait 100 (plus grands restes).
fn energy_split(t: &[f64; 8]) -> EnergySplit {
    let kcal = [
        t[PROTEIN] * 4.0,
        t[CARBS] * 4.0,
        t[FAT] * 9.0,
        t[FIBER] * 2.0,
    ];
    let total: f64 = kcal.iter().sum();
    if total <= 0.0 {
        return EnergySplit::default();
    }
    let exact = kcal.map(|k| k * 100.0 / total);
    let mut pct = exact.map(|x| x.floor() as i32);
    let rest = 100 - pct.iter().sum::<i32>();
    let mut order = [0, 1, 2, 3];
    order.sort_by(|&i, &j| (exact[j] - exact[j].floor()).total_cmp(&(exact[i] - exact[i].floor())));
    for i in order.into_iter().take(rest.max(0) as usize) {
        pct[i] += 1;
    }
    EnergySplit {
        protein: pct[0],
        carbs: pct[1],
        fat: pct[2],
        fiber: pct[3],
    }
}

/// Les apports de la portion (`portions` : le barème d'un membre ; sans, la portion standard) et la note de la
/// recette, toujours sur la portion standard.
pub fn compute(lines: &[Line], portions: Option<&Portions>) -> Nutrition {
    let (mine, missing, blocks) = weigh(lines, portions);
    let (standard, _, _) = weigh(lines, None);
    let t = totals(&mine);
    let f = |x: f64| x as f32;

    let cooked: f64 = standard.iter().map(|w| w.cooked_g).sum();
    let (grade, lowers, holds) = if blocks || cooked <= 0.0 {
        (None, Vec::new(), Vec::new())
    } else {
        let s = totals(&standard);
        let raw: f64 = standard.iter().map(|w| w.raw_g).sum();
        let fvl: f64 = standard
            .iter()
            .filter(|w| w.is_fvl())
            .map(|w| w.raw_g)
            .sum();
        let red: f64 = standard
            .iter()
            .filter(|w| is_red_meat(&w.food.subgroup, &w.food.name_key))
            .map(|w| w.amount(PROTEIN))
            .sum();
        let per100 = |i: usize| s[i] * 100.0 / cooked;
        let pts = points(
            &Per100g {
                energy_kj: per100(ENERGY) * KJ_PER_KCAL,
                sugars_g: per100(SUGARS),
                sat_fat_g: per100(SAT_FAT),
                salt_g: per100(SALT),
                protein_g: per100(PROTEIN),
                fiber_g: per100(FIBER),
                fvl_pct: if raw > 0.0 { fvl * 100.0 / raw } else { 0.0 },
            },
            red * 2.0 > s[PROTEIN],
        );
        let of = |i: usize| top(&mine, |w| w.amount(i));
        let lowers = factors(vec![
            (Criterion::Energy, pts.energy, of(ENERGY)),
            (Criterion::Sugars, pts.sugars, of(SUGARS)),
            (Criterion::SatFat, pts.sat_fat, of(SAT_FAT)),
            (Criterion::Salt, pts.salt, of(SALT)),
        ]);
        let protein = if pts.counts_protein() { pts.protein } else { 0 };
        let holds = factors(vec![
            (Criterion::Protein, protein, of(PROTEIN)),
            (Criterion::Fiber, pts.fiber, of(FIBER)),
            (
                Criterion::Fvl,
                pts.fvl,
                top(&mine, |w| if w.is_fvl() { w.raw_g } else { 0.0 }),
            ),
        ]);
        (Some(super::score::grade(pts.final_score())), lowers, holds)
    };

    let serve = [
        (PortionRole::Protein, "protéines"),
        (PortionRole::Starch, "féculents"),
        (PortionRole::Veg, "légumes"),
    ]
    .into_iter()
    .filter_map(|(role, plural)| {
        let of_role: Vec<&Weighed> = mine.iter().filter(|w| w.line.role == role).collect();
        let first = of_role.first()?;
        Some(Serve {
            role,
            label: if of_role.len() == 1 {
                first.line.name.clone()
            } else {
                plural.to_owned()
            },
            grams: of_role.iter().map(|w| w.cooked_g).sum::<f64>().round() as i32,
        })
    })
    .collect();

    let pct = |i: usize| (t[i] * 100.0 / REFERENCE[i]).round() as i32;
    Nutrition {
        grade,
        portion: if portions.is_some() {
            PortionKind::Member
        } else {
            PortionKind::Standard
        },
        amounts: Amounts {
            energy_kcal: f(t[ENERGY]),
            protein_g: f(t[PROTEIN]),
            carbs_g: f(t[CARBS]),
            sugars_g: f(t[SUGARS]),
            fat_g: f(t[FAT]),
            sat_fat_g: f(t[SAT_FAT]),
            fiber_g: f(t[FIBER]),
            salt_g: f(t[SALT]),
        },
        daily_pct: DailyPct {
            energy_kcal: pct(ENERGY),
            protein_g: pct(PROTEIN),
            carbs_g: pct(CARBS),
            sugars_g: pct(SUGARS),
            fat_g: pct(FAT),
            sat_fat_g: pct(SAT_FAT),
            fiber_g: pct(FIBER),
            salt_g: pct(SALT),
        },
        energy_split: energy_split(&t),
        cooked_g: mine.iter().map(|w| w.cooked_g).sum::<f64>().round() as i32,
        serve,
        lowers,
        holds,
        complete: missing.is_empty(),
        missing,
        to_taste: lines
            .iter()
            .filter(|l| !l.optional && l.qty.is_none())
            .map(|l| l.name.clone())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        nutrition::{Criterion, PortionKind},
        types::Grade,
    };

    /// Teneurs Ciqual pour 100 g : énergie kcal, protéines, glucides, sucres, lipides, saturés, fibres, sel.
    fn food(subgroup: &str, name_key: &str, a: [f32; 8]) -> Option<Food> {
        Some(Food {
            subgroup: subgroup.to_owned(),
            name_key: name_key.to_owned(),
            amounts: a.map(Some),
        })
    }

    fn line(
        name: &str,
        role: PortionRole,
        qty: Option<f32>,
        unit: Unit,
        food: Option<Food>,
    ) -> Line {
        Line {
            name: name.to_owned(),
            role,
            state: IngredientState::Fresh,
            qty,
            unit,
            optional: false,
            per_batch: false,
            piece_g: None,
            density: None,
            cooked_ratio: None,
            food,
        }
    }

    // 6270
    fn paleron() -> Line {
        line(
            "Paleron de bœuf",
            PortionRole::Protein,
            Some(150.0),
            Unit::G,
            food(
                "0402",
                "boeuf, paleron cru",
                [144.0, 21.2, 0.0, 0.0, 6.54, 2.59, 0.0, 0.12],
            ),
        )
    }

    fn cube(per_batch: bool, qty: f32) -> Line {
        // 11001
        Line {
            per_batch,
            piece_g: Some(10.0),
            ..line(
                "Cube de bouillon",
                PortionRole::Other,
                Some(qty),
                Unit::Piece,
                food(
                    "1003",
                    "bouillon de boeuf, deshydrate",
                    [232.0, 10.2, 18.0, 2.0, 13.3, 7.03, 0.0, 33.0],
                ),
            )
        }
    }

    fn boeuf_carottes() -> Vec<Line> {
        vec![
            paleron(),
            // 20009, 20056, 4008, 20034
            line(
                "Carotte",
                PortionRole::Veg,
                Some(200.0),
                Unit::G,
                food(
                    "0201",
                    "carotte, crue",
                    [30.2, 0.78, 5.16, 4.85, 0.5, 0.15, 2.9, 0.12],
                ),
            ),
            line(
                "Champignon",
                PortionRole::Veg,
                Some(100.0),
                Unit::G,
                food(
                    "0201",
                    "champignon de paris",
                    [21.0, 2.11, 1.83, 1.55, 0.36, 0.067, 1.0, 0.098],
                ),
            ),
            line(
                "Pomme de terre",
                PortionRole::Starch,
                Some(150.0),
                Unit::G,
                food(
                    "0202",
                    "pomme de terre, sans peau, crue",
                    [80.0, 2.02, 16.2, 0.78, 0.09, 0.026, 2.2, 0.0011],
                ),
            ),
            Line {
                piece_g: Some(100.0),
                ..line(
                    "Oignon",
                    PortionRole::Veg,
                    Some(0.5),
                    Unit::Piece,
                    food(
                        "0201",
                        "oignon, cru",
                        [39.0, 1.1, 6.25, 4.8, 0.62, 0.2, 1.7, 0.098],
                    ),
                )
            },
            // 17270
            Line {
                density: Some(0.92),
                ..line(
                    "Huile d'olive",
                    PortionRole::Fat,
                    Some(0.25),
                    Unit::Tbsp,
                    food(
                        "0902",
                        "huile d'olive vierge extra",
                        [899.0, 0.25, 0.0, 0.0, 99.9, 15.2, 0.0, 0.013],
                    ),
                )
            },
            cube(true, 1.0),
            // 11058
            line(
                "Sel",
                PortionRole::Other,
                None,
                Unit::Pinch,
                food(
                    "1004",
                    "sel blanc alimentaire",
                    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 97.8],
                ),
            ),
        ]
    }

    fn close(actual: f32, expected: f32, what: &str) {
        assert!(
            (actual - expected).abs() < 0.5,
            "{what} : {actual} au lieu de {expected}"
        );
    }

    #[test]
    fn boeuf_carottes_portion_standard() {
        let n = compute(&boeuf_carottes(), None);
        assert_eq!(n.portion, PortionKind::Standard);
        close(n.amounts.energy_kcal, 473.7, "énergie");
        close(n.amounts.protein_g, 39.3, "protéines");
        close(n.amounts.salt_g, 1.39, "sel");
        assert_eq!(n.to_taste, vec!["Sel".to_owned()]);
        assert!(n.complete && n.missing.is_empty());
        assert_eq!(n.cooked_g, 583);
        let serve: Vec<(PortionRole, &str, i32)> = n
            .serve
            .iter()
            .map(|s| (s.role, s.label.as_str(), s.grams))
            .collect();
        assert_eq!(
            serve,
            vec![
                (PortionRole::Protein, "Paleron de bœuf", 113),
                (PortionRole::Starch, "Pomme de terre", 150),
                (PortionRole::Veg, "légumes", 315),
            ]
        );
        // Pour 100 g cuits : 340 kJ (1), sucres 2,5 (0), saturés 0,9 (0), sel 0,24 (1) ; protéines 6,7 g de
        // viande rouge (2 au plus), fibres 1,9 (0), fruits et légumes 53 % (1) : 2 − 3 = −1.
        assert_eq!(n.grade, Some(Grade::A));
        assert_eq!(n.daily_pct.energy_kcal, 24);
        assert_eq!(n.daily_pct.protein_g, 79);
    }

    #[test]
    fn portion_du_membre() {
        let teen = Portions {
            protein: 1.0,
            starch: 2.0,
            veg: 1.0,
            fat: 1.0,
            other: 1.0,
        };
        let standard = compute(&boeuf_carottes(), None);
        let member = compute(&boeuf_carottes(), Some(&teen));
        assert_eq!(member.portion, PortionKind::Member);
        assert!(member.amounts.carbs_g > standard.amounts.carbs_g + 20.0);
        assert_eq!(member.grade, standard.grade, "la note juge la recette");
        assert_eq!(member.serve[1].grams, 300);
    }

    #[test]
    fn recette_sans_poids_sans_note() {
        let lines = vec![
            line("Sel", PortionRole::Other, None, Unit::Pinch, None),
            Line {
                optional: true,
                ..paleron()
            },
        ];
        let n = compute(&lines, None);
        assert_eq!(n.grade, None);
        assert_eq!(n.amounts, Default::default());
        assert_eq!(n.energy_split, Default::default());
        assert_eq!(n.cooked_g, 0);
        assert!(n.serve.is_empty());
    }

    #[test]
    fn epice_sans_donnees_ne_bloque_pas() {
        let mut lines = boeuf_carottes();
        lines.push(line(
            "Ras el-hanout",
            PortionRole::Other,
            Some(1.0),
            Unit::Tsp,
            None,
        ));
        let n = compute(&lines, None);
        assert_eq!(n.missing, vec!["Ras el-hanout".to_owned()]);
        assert!(!n.complete);
        assert!(n.grade.is_some());
    }

    #[test]
    fn piece_sans_poids_bloque() {
        let mut lines = boeuf_carottes();
        lines.push(line(
            "Citron",
            PortionRole::Other,
            Some(1.0),
            Unit::Piece,
            food(
                "0204",
                "citron, pulpe, cru",
                [37.0, 0.5, 6.0, 2.0, 0.3, 0.0, 1.0, 0.0],
            ),
        ));
        let n = compute(&lines, None);
        assert_eq!(n.missing, vec!["Citron".to_owned()]);
        assert_eq!(n.grade, None);
    }

    #[test]
    fn teneur_vide_compte_zero() {
        let mut beef = paleron();
        beef.food.as_mut().unwrap().amounts[5] = None;
        let n = compute(&[beef], None);
        assert_eq!(n.amounts.sat_fat_g, 0.0);
        assert!(n.amounts.energy_kcal.is_finite() && n.grade.is_some());
    }

    #[test]
    fn repartition_fait_cent() {
        for lines in [boeuf_carottes(), vec![paleron()], vec![cube(false, 3.0)]] {
            let s = compute(&lines, None).energy_split;
            assert_eq!(s.protein + s.carbs + s.fat + s.fiber, 100, "{s:?}");
        }
    }

    #[test]
    fn rendements_par_defaut() {
        assert_eq!(default_cooked_ratio("0301", IngredientState::Dried), 2.5);
        assert_eq!(default_cooked_ratio("0301", IngredientState::Cooked), 1.0);
        assert_eq!(default_cooked_ratio("0203", IngredientState::Dried), 2.5);
        assert_eq!(default_cooked_ratio("0203", IngredientState::Canned), 1.0);
        assert_eq!(default_cooked_ratio("0402", IngredientState::Fresh), 0.75);
        assert_eq!(default_cooked_ratio("0406", IngredientState::Frozen), 0.8);
        assert_eq!(default_cooked_ratio("0403", IngredientState::Fresh), 0.9);
        assert_eq!(default_cooked_ratio("0201", IngredientState::Fresh), 0.9);
        assert_eq!(default_cooked_ratio("0201", IngredientState::Canned), 1.0);
        assert_eq!(default_cooked_ratio("0904", IngredientState::Fresh), 1.0);

        let rice = |cooked_ratio| Line {
            state: IngredientState::Dried,
            cooked_ratio,
            ..line(
                "Riz",
                PortionRole::Starch,
                Some(100.0),
                Unit::G,
                food(
                    "0301",
                    "riz blanc, cru",
                    [350.0, 7.02, 77.5, 0.12, 0.79, 0.16, 1.53, 0.0079],
                ),
            )
        };
        assert_eq!(compute(&[rice(None)], None).cooked_g, 250);
        assert_eq!(compute(&[rice(Some(2.4))], None).cooked_g, 240);
    }

    #[test]
    fn viande_rouge() {
        assert!(is_red_meat("0402", "boeuf, paleron cru"));
        assert!(!is_red_meat("0402", "poulet, filet, sans peau, cru"));
        assert!(is_red_meat("0403", "jambon cuit, choix"));
        assert!(!is_red_meat(
            "0403",
            "jambon de dinde ou blanc de dinde en tranche"
        ));
        assert!(!is_red_meat("0406", "saumon, cru"));
    }

    #[test]
    fn sel_baisse_la_note() {
        let mut lines = boeuf_carottes();
        lines.retain(|l| l.name != "Cube de bouillon");
        lines.push(cube(false, 2.0));
        let n = compute(&lines, None);
        let salt = &n.lowers[0];
        assert_eq!(salt.criterion, Criterion::Salt);
        assert_eq!(salt.ingredient.as_deref(), Some("Cube de bouillon"));
        close(salt.amount.unwrap(), 6.6, "sel du cube");
    }
}
