//! L'algorithme du Nutri-Score pour les aliments généraux, tel que mis à jour en 2023-2024 : *Update of the
//! Nutri-Score algorithm, Yearly report from the Scientific Committee of the Nutri-Score, 2022*, annexe § 1 « Recap
//! of the update in the main algorithm ». Les tables en sont recopiées : ne pas les modifier sans le texte.

use crate::types::Grade;

// Points : nombre de seuils strictement dépassés.
const ENERGY_KJ: &[f64] = &[
    335.0, 670.0, 1005.0, 1340.0, 1675.0, 2010.0, 2345.0, 2680.0, 3015.0, 3350.0,
];
const SUGARS_G: &[f64] = &[
    3.4, 6.8, 10.0, 14.0, 17.0, 20.0, 24.0, 27.0, 31.0, 34.0, 37.0, 41.0, 44.0, 48.0, 51.0,
];
const SAT_FAT_G: &[f64] = &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
const SALT_G: &[f64] = &[
    0.2, 0.4, 0.6, 0.8, 1.0, 1.2, 1.4, 1.6, 1.8, 2.0, 2.2, 2.4, 2.6, 2.8, 3.0, 3.2, 3.4, 3.6, 3.8,
    4.0,
];
const PROTEIN_G: &[f64] = &[2.4, 4.8, 7.2, 9.6, 12.0, 14.0, 17.0];
const FIBER_G: &[f64] = &[3.0, 4.1, 5.2, 6.3, 7.4];
/// Points de protéines au plus pour la viande rouge (bœuf, veau, porc, agneau, gibier…).
const RED_MEAT_PROTEIN_MAX: i32 = 2;
/// Au-delà, les protéines ne comptent plus (l'exception des fromages ne vise pas un plat).
const PROTEIN_CUTOFF: i32 = 11;

/// Teneurs pour 100 g, et part de fruits, légumes et légumineuses (%).
#[derive(Clone, Copy, Debug)]
pub struct Per100g {
    pub energy_kj: f64,
    pub sugars_g: f64,
    pub sat_fat_g: f64,
    pub salt_g: f64,
    pub protein_g: f64,
    pub fiber_g: f64,
    pub fvl_pct: f64,
}

/// Points de chaque composant : les quatre premiers défavorables, les trois derniers favorables.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Points {
    pub energy: i32,
    pub sugars: i32,
    pub sat_fat: i32,
    pub salt: i32,
    pub protein: i32,
    pub fiber: i32,
    pub fvl: i32,
}

fn scale(value: f64, thresholds: &[f64]) -> i32 {
    thresholds.iter().filter(|t| value > **t).count() as i32
}

pub fn points(p: &Per100g, red_meat: bool) -> Points {
    let protein = scale(p.protein_g, PROTEIN_G);
    Points {
        energy: scale(p.energy_kj, ENERGY_KJ),
        sugars: scale(p.sugars_g, SUGARS_G),
        sat_fat: scale(p.sat_fat_g, SAT_FAT_G),
        salt: scale(p.salt_g, SALT_G),
        protein: if red_meat {
            protein.min(RED_MEAT_PROTEIN_MAX)
        } else {
            protein
        },
        fiber: scale(p.fiber_g, FIBER_G),
        fvl: match p.fvl_pct {
            x if x > 80.0 => 5,
            x if x > 60.0 => 2,
            x if x > 40.0 => 1,
            _ => 0,
        },
    }
}

impl Points {
    /// Les points défavorables (« points A »).
    pub fn negative(&self) -> i32 {
        self.energy + self.sugars + self.sat_fat + self.salt
    }

    /// Les protéines comptent-elles ?
    pub fn counts_protein(&self) -> bool {
        self.negative() < PROTEIN_CUTOFF
    }

    pub fn final_score(&self) -> i32 {
        let protein = if self.counts_protein() {
            self.protein
        } else {
            0
        };
        self.negative() - (protein + self.fiber + self.fvl)
    }
}

pub fn grade(score: i32) -> Grade {
    match score {
        i32::MIN..=0 => Grade::A,
        1..=2 => Grade::B,
        3..=10 => Grade::C,
        11..=18 => Grade::D,
        _ => Grade::E,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> Per100g {
        Per100g {
            energy_kj: 0.0,
            sugars_g: 0.0,
            sat_fat_g: 0.0,
            salt_g: 0.0,
            protein_g: 0.0,
            fiber_g: 0.0,
            fvl_pct: 0.0,
        }
    }

    #[test]
    fn seuil_strict() {
        let at = |p: Per100g| points(&p, false);
        assert_eq!(
            at(Per100g {
                energy_kj: 335.0,
                ..empty()
            })
            .energy,
            0
        );
        assert_eq!(
            at(Per100g {
                energy_kj: 335.1,
                ..empty()
            })
            .energy,
            1
        );
        assert_eq!(
            at(Per100g {
                energy_kj: 5000.0,
                ..empty()
            })
            .energy,
            10
        );
        assert_eq!(
            at(Per100g {
                sugars_g: 51.5,
                ..empty()
            })
            .sugars,
            15
        );
        assert_eq!(
            at(Per100g {
                sat_fat_g: 1.0,
                ..empty()
            })
            .sat_fat,
            0
        );
        assert_eq!(
            at(Per100g {
                sat_fat_g: 10.5,
                ..empty()
            })
            .sat_fat,
            10
        );
        assert_eq!(
            at(Per100g {
                salt_g: 0.2,
                ..empty()
            })
            .salt,
            0
        );
        assert_eq!(
            at(Per100g {
                salt_g: 0.55,
                ..empty()
            })
            .salt,
            2
        );
        assert_eq!(
            at(Per100g {
                salt_g: 4.01,
                ..empty()
            })
            .salt,
            20
        );
        assert_eq!(
            at(Per100g {
                fiber_g: 7.4,
                ..empty()
            })
            .fiber,
            4
        );
        assert_eq!(
            at(Per100g {
                fiber_g: 7.5,
                ..empty()
            })
            .fiber,
            5
        );
    }

    #[test]
    fn fvl_saute_a_cinq() {
        let fvl = |pct: f64| {
            points(
                &Per100g {
                    fvl_pct: pct,
                    ..empty()
                },
                false,
            )
            .fvl
        };
        assert_eq!(fvl(40.0), 0);
        assert_eq!(fvl(40.1), 1);
        assert_eq!(fvl(60.1), 2);
        assert_eq!(fvl(80.0), 2);
        assert_eq!(fvl(80.1), 5);
    }

    #[test]
    fn viande_rouge_plafonnee() {
        let p = Per100g {
            protein_g: 20.0,
            ..empty()
        };
        assert_eq!(points(&p, false).protein, 7);
        assert_eq!(points(&p, true).protein, 2);
    }

    #[test]
    fn proteines_ignorees_au_dela_de_onze() {
        let base = Points {
            energy: 4,
            sugars: 2,
            sat_fat: 3,
            salt: 2,
            protein: 7,
            fiber: 2,
            fvl: 0,
        };
        assert_eq!(base.negative(), 11);
        assert_eq!(base.final_score(), 9);
        let under = Points { salt: 1, ..base };
        assert_eq!(under.negative(), 10);
        assert_eq!(under.final_score(), 1);
    }

    #[test]
    fn classes() {
        let cases = [
            (-5, Grade::A),
            (0, Grade::A),
            (1, Grade::B),
            (2, Grade::B),
            (3, Grade::C),
            (10, Grade::C),
            (11, Grade::D),
            (18, Grade::D),
            (19, Grade::E),
        ];
        for (score, expected) in cases {
            assert_eq!(grade(score), expected, "{score}");
        }
    }
}
