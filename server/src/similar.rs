//! Recettes proches : deux recettes se ressemblent si elles partagent leurs ingrédients, la protéine et le féculent
//! d'abord, puis leur nom et leur matériel. Sert à demander « c'en est une version ? » ; jamais à décider seul.

use std::collections::{HashMap, HashSet};

use crate::{text::search_key, types::PortionRole};

/// Ce qu'on compare d'une recette.
#[derive(Debug, Default)]
pub struct Profile {
    /// Mots du titre (sans accents ni petits mots).
    words: HashSet<String>,
    /// Ingrédients par code, avec le poids de leur rôle.
    ingredients: HashMap<String, f32>,
    equipment: HashSet<String>,
}

/// La protéine et le féculent font le plat ; les légumes un peu ; le gras et le reste (épices, ail) presque pas.
fn weight(role: PortionRole) -> f32 {
    match role {
        PortionRole::Protein => 3.0,
        PortionRole::Starch => 2.0,
        PortionRole::Veg => 1.0,
        PortionRole::Fat | PortionRole::Other => 0.5,
    }
}

const SMALL_WORDS: &[&str] = &[
    "a", "au", "aux", "de", "des", "du", "d", "en", "et", "l", "la", "le", "les", "sur", "un",
    "une", "avec",
];

impl Profile {
    pub fn new<'a>(
        title: &str,
        lines: impl IntoIterator<Item = (&'a str, PortionRole)>,
        equipment: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        let mut ingredients: HashMap<String, f32> = HashMap::new();
        for (code, role) in lines {
            let w = ingredients.entry(code.to_owned()).or_default();
            *w = w.max(weight(role));
        }
        Self {
            words: search_key(title)
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty() && !SMALL_WORDS.contains(w))
                .map(str::to_owned)
                .collect(),
            ingredients,
            equipment: equipment.into_iter().map(str::to_owned).collect(),
        }
    }
}

/// La part commune, moitié rapportée à l'ensemble des deux (Jaccard), moitié à la plus petite des deux : un brouillon
/// court n'est pas écrasé par une recette riche (le curry de la semaine face à celui aux quatorze ingrédients).
fn overlap(shared: f32, a: f32, b: f32) -> f32 {
    let union = a + b - shared;
    let smaller = a.min(b);
    if union <= 0.0 || smaller <= 0.0 {
        return 0.0;
    }
    (shared / union + shared / smaller) / 2.0
}

fn sets(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    overlap(
        a.intersection(b).count() as f32,
        a.len() as f32,
        b.len() as f32,
    )
}

/// De 0 à 1 : 60 % les ingrédients (pondérés par rôle), 30 % le nom, 10 % le matériel. Et les ingrédients en commun,
/// du plus important au moins important.
pub fn score(a: &Profile, b: &Profile) -> (f32, Vec<String>) {
    let mut common: Vec<(&String, f32)> = Vec::new();
    for (code, &wa) in &a.ingredients {
        if let Some(&wb) = b.ingredients.get(code) {
            common.push((code, wa.max(wb)));
        }
    }
    let shared: f32 = common.iter().map(|(_, w)| w).sum();
    let ingredients = overlap(
        shared,
        a.ingredients.values().sum(),
        b.ingredients.values().sum(),
    );
    let value =
        0.6 * ingredients + 0.3 * sets(&a.words, &b.words) + 0.1 * sets(&a.equipment, &b.equipment);
    common.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(y.0)));
    (
        value,
        common.into_iter().map(|(code, _)| code.clone()).collect(),
    )
}

/// À partir de là, on demande « c'en est une version ? ».
pub const THRESHOLD: f32 = 0.5;

#[cfg(test)]
mod tests {
    use super::*;
    use PortionRole::*;

    fn p(title: &str, lines: &[(&str, PortionRole)], equipment: &[&str]) -> Profile {
        Profile::new(title, lines.iter().copied(), equipment.iter().copied())
    }

    /// Les plats communs des menus de septembre-octobre, écrits deux fois avant d'être fusionnés à la main : chaque
    /// paire se ressemble, et aucun plat ne ressemble à un autre plat.
    #[test]
    fn recettes_proches_des_plats_communs() {
        let curry = p(
            "Curry de poulet, patate douce et pois chiches",
            &[
                ("chicken", Protein),
                ("sweet_potato", Starch),
                ("chickpea", Veg),
                ("coconut_milk", Fat),
                ("rice", Starch),
            ],
            &["cookeo_6l"],
        );
        let curry_septembre = p(
            "Curry de poulet aux épinards",
            &[
                ("chicken", Protein),
                ("chickpea", Veg),
                ("spinach", Veg),
                ("coconut_milk", Fat),
                ("rice", Starch),
            ],
            &["cookeo_6l"],
        );
        // Le curry du catalogue, riche de quatorze ingrédients, face à un brouillon de quatre.
        let curry_riche = p(
            "Curry de poulet, patate douce & pois chiches, riz basmati",
            &[
                ("chicken", Protein),
                ("sweet_potato", Veg),
                ("chickpea", Veg),
                ("coconut_milk", Fat),
                ("tomato", Veg),
                ("onion", Veg),
                ("garlic", Other),
                ("ginger", Other),
                ("curry_paste", Other),
                ("rice", Starch),
                ("olive_oil", Fat),
                ("coriander", Other),
                ("spinach", Veg),
                ("bread", Starch),
            ],
            &["cookeo_6l", "thermomix_tm6", "hob"],
        );
        let curry_court = p(
            "Curry de poulet aux épinards",
            &[
                ("chicken", Protein),
                ("chickpea", Veg),
                ("spinach", Veg),
                ("rice", Starch),
            ],
            &["cookeo_6l"],
        );
        let chili = p(
            "Chili con carne",
            &[
                ("minced_beef", Protein),
                ("kidney_bean", Veg),
                ("tomato", Veg),
                ("rice", Starch),
            ],
            &["cookeo_6l"],
        );
        let chili_bis = p(
            "Chili con carne, riz basmati",
            &[
                ("minced_beef", Protein),
                ("kidney_bean", Veg),
                ("tomato", Veg),
                ("corn", Veg),
                ("rice", Starch),
            ],
            &["cookeo_6l", "hob"],
        );
        let poulet = p(
            "Poulet rôti, pommes de terre",
            &[("chicken", Protein), ("potato", Starch)],
            &["oven"],
        );
        let poulet_bis = p(
            "Poulet rôti et ses pommes de terre au four",
            &[("chicken", Protein), ("potato", Starch), ("garlic", Other)],
            &["oven"],
        );
        let saumon = p(
            "Saumon poêlé, riz",
            &[("salmon", Protein), ("rice", Starch), ("lemon", Other)],
            &["hob"],
        );
        let saumon_bis = p(
            "Saumon poêlé, riz et brocolis",
            &[("salmon", Protein), ("rice", Starch), ("broccoli", Veg)],
            &["hob"],
        );
        let poisson = p(
            "Poisson rôti, légumes",
            &[("cod", Protein), ("zucchini", Veg), ("potato", Starch)],
            &["oven"],
        );
        let poisson_bis = p(
            "Poisson rôti au four",
            &[
                ("cod", Protein),
                ("zucchini", Veg),
                ("tomato", Veg),
                ("potato", Starch),
            ],
            &["oven"],
        );
        let pairs = [
            (&curry, &curry_septembre),
            (&curry_riche, &curry_court),
            (&chili, &chili_bis),
            (&poulet, &poulet_bis),
            (&saumon, &saumon_bis),
            (&poisson, &poisson_bis),
        ];
        for (a, b) in pairs {
            assert!(
                score(a, b).0 >= THRESHOLD,
                "{:?} / {:?} : {}",
                a.words,
                b.words,
                score(a, b).0
            );
        }
        let dishes = [&curry, &chili, &poulet, &saumon, &poisson];
        for (i, a) in dishes.iter().enumerate() {
            for b in &dishes[i + 1..] {
                assert!(
                    score(a, b).0 < THRESHOLD,
                    "{:?} / {:?} : {}",
                    a.words,
                    b.words,
                    score(a, b).0
                );
            }
        }
        // Les ingrédients en commun, la protéine d'abord.
        assert_eq!(score(&curry, &curry_septembre).1[0], "chicken");
    }
}
