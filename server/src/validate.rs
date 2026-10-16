//! Validation des champs saisis.

use crate::error::{ApiError, ApiResult};

/// Nom d'un foyer, d'un membre, d'un compte : 1 à 80 caractères, espaces autour retirés.
pub fn name(value: &str) -> ApiResult<&str> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 80 {
        return Err(ApiError::invalid(
            "Le nom doit faire entre 1 et 80 caractères.",
        ));
    }
    Ok(value)
}

/// Fuseau horaire IANA (ex. `Europe/Paris`). Forme seulement : le serveur n'embarque pas la base des fuseaux.
pub fn timezone(value: &str) -> ApiResult<&str> {
    let value = value.trim();
    let ok = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_-+".contains(&b));
    if !ok {
        return Err(ApiError::invalid("Fuseau horaire invalide."));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noms() {
        assert_eq!(name("  Foyer  ").unwrap(), "Foyer");
        assert!(name("   ").is_err());
        assert!(name(&"é".repeat(80)).is_ok());
        assert!(name(&"é".repeat(81)).is_err());
    }

    #[test]
    fn fuseaux() {
        assert!(timezone("Europe/Paris").is_ok());
        assert!(timezone("America/Port-au-Prince").is_ok());
        assert!(timezone("Europe/Paris; DROP").is_err());
    }
}
