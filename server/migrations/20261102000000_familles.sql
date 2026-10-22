-- M5 : familles de recettes. Les déclinaisons d'un même plat (gratin dauphinois classique, à la crème, savoyard)
-- forment une famille ; chaque foyer y a sa préférée, proposée en premier. Jamais rien d'automatique : c'est l'auteur
-- qui range une recette dans une famille (l'admin, pour une recette de la communauté).
CREATE TABLE recipe_families (
    id         uuid PRIMARY KEY,
    title      text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE recipes ADD COLUMN family_id uuid REFERENCES recipe_families ON DELETE SET NULL;
CREATE INDEX recipes_family_id ON recipes (family_id);

-- La préférée d'un foyer dans une famille. Par foyer pour l'instant ; par personne plus tard = une colonne
-- `member_id` et une clé élargie, sans toucher aux familles.
CREATE TABLE family_preferences (
    household_id uuid NOT NULL REFERENCES households ON DELETE CASCADE,
    family_id    uuid NOT NULL REFERENCES recipe_families ON DELETE CASCADE,
    recipe_id    uuid NOT NULL REFERENCES recipes ON DELETE CASCADE,
    PRIMARY KEY (household_id, family_id)
);

-- Une recette proposée comme déclinaison d'une recette de la communauté : l'admin range les deux ensemble en
-- l'acceptant.
ALTER TABLE recipe_submissions ADD COLUMN family_with uuid REFERENCES recipes ON DELETE SET NULL;
