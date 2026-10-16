//! Codes partagés entre l'API et la base, stockés en `text` (avec une contrainte CHECK côté SQL).

/// Déclare une énumération sérialisée par ses codes, lisible depuis une colonne `text` avec
/// `#[sqlx(try_from = "String")]` et écrite avec `as_str()`.
macro_rules! text_enum {
    ($(#[$meta:meta])* pub enum $name:ident { $($(#[$vmeta:meta])* $variant:ident = $text:literal,)+ }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
        pub enum $name {
            $($(#[$vmeta])* #[serde(rename = $text)] #[schema(rename = $text)] $variant,)+
        }

        impl $name {
            pub const ALL: &[Self] = &[$(Self::$variant,)+];

            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)+
                }
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;

            fn try_from(value: String) -> Result<Self, String> {
                match value.as_str() {
                    $($text => Ok(Self::$variant),)+
                    _ => Err(format!("{} inconnu : {value}", stringify!($name))),
                }
            }
        }
    };
}

text_enum! {
    /// Rôle d'un compte dans un foyer.
    pub enum Role {
        /// Gère aussi les comptes : invitations, rôles, suppression du foyer.
        Owner = "owner",
        /// Voit et modifie tout ce qui concerne la cuisine.
        Member = "member",
    }
}

text_enum! {
    /// Type de mangeur : fixe le barème par défaut.
    pub enum MemberKind {
        Adult = "adult",
        Teen = "teen",
        Child = "child",
    }
}

text_enum! {
    /// Régime alimentaire.
    pub enum Diet {
        Omnivore = "omnivore",
        Pescatarian = "pescatarian",
        Vegetarian = "vegetarian",
        Vegan = "vegan",
    }
}

text_enum! {
    /// Repas de la journée.
    pub enum Meal {
        Lunch = "lunch",
        Dinner = "dinner",
    }
}

text_enum! {
    /// `allergy` : filtre dur ; `dislike` : à éviter.
    pub enum ExclusionKind {
        Allergy = "allergy",
        Dislike = "dislike",
    }
}

text_enum! {
    /// Forme d'achat d'un ingrédient : rayon des courses et ligne Ciqual pour les apports.
    pub enum IngredientState {
        /// Frais, ou tel quel (moutarde, lait).
        Fresh = "fresh",
        Frozen = "frozen",
        Canned = "canned",
        /// Sec ou déshydraté (pâtes, riz, lentilles, fruits à coque).
        Dried = "dried",
        /// Acheté cuit (crevettes, betteraves, riz précuit).
        Cooked = "cooked",
    }
}

text_enum! {
    /// Rayon du magasin.
    pub enum Aisle {
        Fresh = "fresh",
        Produce = "produce",
        Butcher = "butcher",
        Fish = "fish",
        Dairy = "dairy",
        Frozen = "frozen",
        Grocery = "grocery",
        Bakery = "bakery",
        Other = "other",
    }
}

text_enum! {
    /// Unité normalisée.
    pub enum Unit {
        G = "g",
        Ml = "ml",
        Piece = "piece",
        Tbsp = "tbsp",
        Tsp = "tsp",
        Pinch = "pinch",
    }
}
