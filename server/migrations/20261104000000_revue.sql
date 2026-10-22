-- M5, revue de sécurité du 22 oct.
-- Une version garde le foyer à qui la recette appartenait quand elle a été écrite : acceptée dans la communauté, la
-- recette garde son histoire, mais les versions d'avant restent visibles de ce foyer seul.
ALTER TABLE recipe_versions ADD COLUMN household_id uuid REFERENCES households ON DELETE CASCADE;
UPDATE recipe_versions v SET household_id = r.household_id FROM recipes r WHERE r.id = v.recipe_id;

-- La version proposée : l'admin accepte ce qu'il a relu, pas une recette modifiée depuis.
ALTER TABLE recipe_submissions ADD COLUMN version integer;
