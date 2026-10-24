-- Apports d'une portion (design-nutrition.md) : rendement de cuisson d'un ingrédient (poids cuit / poids cru), pour
-- le poids à servir et la note sur 100 g de plat cuit ; nul : le défaut du code, selon son groupe Ciqual.
ALTER TABLE ingredients ADD COLUMN cooked_ratio real CHECK (cooked_ratio > 0);
