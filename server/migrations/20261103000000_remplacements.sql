-- M5 : remplacements d'ingrédients. « Lieu noir pour cabillaud, quantité × 1,1 » : saisi par l'admin de l'instance,
-- ou issu d'une variante tentée validée ; proposé dans toutes les recettes qui ont l'ingrédient, sauf celles qui le
-- refusent. Une recette peut aussi le préciser (« +2 min de vapeur »).
CREATE TABLE substitutions (
    id               uuid PRIMARY KEY,
    ingredient_id    uuid NOT NULL REFERENCES ingredients ON DELETE CASCADE,
    substitute_id    uuid NOT NULL REFERENCES ingredients ON DELETE CASCADE,
    -- Forme d'achat du remplaçant ; nulle : celle de la ligne remplacée.
    substitute_state text CHECK (substitute_state IN ('fresh', 'frozen', 'canned', 'dried', 'cooked')),
    factor           real NOT NULL DEFAULT 1 CHECK (factor > 0 AND factor <= 10),
    note             text NOT NULL DEFAULT '',
    source           text NOT NULL CHECK (source IN ('admin', 'feedback')),
    feedback_id      uuid REFERENCES recipe_feedback ON DELETE SET NULL,
    created_at       timestamptz NOT NULL DEFAULT now(),
    UNIQUE (ingredient_id, substitute_id),
    CHECK (ingredient_id <> substitute_id)
);

CREATE TABLE recipe_substitutions (
    recipe_id       uuid NOT NULL REFERENCES recipes ON DELETE CASCADE,
    substitution_id uuid NOT NULL REFERENCES substitutions ON DELETE CASCADE,
    allowed         boolean NOT NULL,
    note            text NOT NULL DEFAULT '',
    PRIMARY KEY (recipe_id, substitution_id)
);

-- Le remplacement choisi sur une ligne de courses : il vaut pour les plats du menu qui la composent (la cuisson le
-- suit). Gardé sur l'article, dont la clé survit à la republication du menu ; rejoué hors ligne comme une case.
ALTER TABLE shopping_items ADD COLUMN substitution_id uuid REFERENCES substitutions ON DELETE SET NULL;
ALTER TABLE shopping_ops DROP CONSTRAINT shopping_ops_kind_check;
ALTER TABLE shopping_ops ADD CONSTRAINT shopping_ops_kind_check
    CHECK (kind IN ('check', 'uncheck', 'add', 'edit', 'remove', 'substitute'));
