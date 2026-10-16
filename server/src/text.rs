//! Normalisation des noms pour la recherche et les codes.

/// Clé de recherche : minuscules, sans accents, œ → oe, æ → ae, espaces simples.
/// Même règle que `searchKey` de `packages/domain` (côté appli).
pub fn search_key(text: &str) -> String {
    let mut key = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            'œ' => key.push_str("oe"),
            'æ' => key.push_str("ae"),
            c if c.is_whitespace() => {
                if !key.is_empty() && !key.ends_with(' ') {
                    key.push(' ');
                }
            }
            c => key.push(unaccent(c)),
        }
    }
    key.truncate(key.trim_end().len());
    key
}

/// Lettre sans son accent (alphabets latins courants) ; les autres caractères restent tels quels.
fn unaccent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        c => c,
    }
}

/// Motifs `LIKE` (un par mot, caractères spéciaux échappés) pour chercher tous les mots dans une clé de recherche.
pub fn like_patterns(query: &str) -> Vec<String> {
    search_key(query)
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let w = w
                .replace('\\', r"\\")
                .replace('%', r"\%")
                .replace('_', r"\_");
            format!("%{w}%")
        })
        .collect()
}

/// Code tiré d'un nom : clé de recherche, lettres et chiffres reliés par `_` (« Crème fraîche » → `creme_fraiche`).
pub fn code(name: &str) -> String {
    search_key(name)
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cles_de_recherche() {
        assert_eq!(search_key("Épinard, CRU"), "epinard, cru");
        assert_eq!(search_key("Bœuf  haché"), "boeuf hache");
        assert_eq!(search_key("Cæcum"), "caecum");
        assert_eq!(
            search_key(" Pâtes, à l'ŒUF, maïs, ç "),
            "pates, a l'oeuf, mais, c"
        );
    }

    #[test]
    fn motifs_de_recherche() {
        assert_eq!(like_patterns(" Bœuf  HACHÉ "), ["%boeuf%", "%hache%"]);
        assert_eq!(like_patterns(r"50%_x\"), [r"%50\%\_x\\%"]);
        assert!(like_patterns("   ").is_empty());
    }

    #[test]
    fn codes() {
        assert_eq!(code("Crème fraîche"), "creme_fraiche");
        assert_eq!(code(" Petits pois "), "petits_pois");
        assert_eq!(code("Bœuf, steak haché 15 %"), "boeuf_steak_hache_15");
        assert_eq!(code("Huile d'olive"), "huile_d_olive");
    }
}
