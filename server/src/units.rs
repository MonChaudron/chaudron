//! Les quantités des recettes en grammes : partagé par les prix et les apports.

use crate::types::Unit;

/// Une cuillère à soupe, une cuillère à café (ml), une pincée (g).
pub const TBSP_ML: f64 = 15.0;
pub const TSP_ML: f64 = 5.0;
pub const PINCH_G: f64 = 0.5;

/// Une quantité en grammes ; les volumes à la densité de l'ingrédient (1 sans : l'erreur est faible devant un trou),
/// les pièces à leur poids (`None` sans).
pub fn grams(qty: f32, unit: Unit, piece_g: Option<f32>, density: Option<f32>) -> Option<f64> {
    let qty = f64::from(qty);
    let density = f64::from(density.unwrap_or(1.0));
    match unit {
        Unit::G => Some(qty),
        Unit::Pinch => Some(qty * PINCH_G),
        Unit::Ml => Some(qty * density),
        Unit::Tbsp => Some(qty * TBSP_ML * density),
        Unit::Tsp => Some(qty * TSP_ML * density),
        Unit::Piece => piece_g.map(|g| qty * f64::from(g)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammes_de_chaque_unite() {
        assert_eq!(grams(150.0, Unit::G, None, None), Some(150.0));
        assert_eq!(grams(2.0, Unit::Pinch, None, None), Some(1.0));
        assert!((grams(1.0, Unit::Tbsp, None, Some(0.92)).unwrap() - 13.8).abs() < 1e-6);
        assert_eq!(grams(1.0, Unit::Tsp, None, None), Some(5.0));
        assert_eq!(grams(0.5, Unit::Piece, Some(100.0), None), Some(50.0));
        assert_eq!(grams(1.0, Unit::Piece, None, None), None);
    }
}
