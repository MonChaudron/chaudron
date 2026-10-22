-- M5 : recettes de la communauté (`household_id` nul, visibles par tous les foyers, modifiées par l'admin de
-- l'instance), clés stables des éléments d'une recette, historique des versions.

-- Un slug de la communauté est unique sur l'instance, et aucun foyer n'en a un pareil (vérifié à l'écriture, sous
-- un verrou sur le slug) : une adresse `/recettes/{slug}` désigne une seule recette pour un foyer.
ALTER TABLE recipes ALTER COLUMN household_id DROP NOT NULL;
ALTER TABLE recipes DROP CONSTRAINT recipes_household_id_slug_key;
CREATE UNIQUE INDEX recipes_household_slug ON recipes (household_id, slug) WHERE household_id IS NOT NULL;
CREATE UNIQUE INDEX recipes_community_slug ON recipes (slug) WHERE household_id IS NULL;
-- Numéro de la version courante ; 0 : pas encore d'historique (la version de départ s'écrit au démarrage).
ALTER TABLE recipes ADD COLUMN version integer NOT NULL DEFAULT 0 CHECK (version >= 0);
-- Recette de la communauté retirée du catalogue : plus proposée, gardée par les menus qui l'utilisent.
ALTER TABLE recipes ADD COLUMN retired_at timestamptz;

-- Une étape, une ligne d'ingrédient garde sa clé d'une version à l'autre (republier recrée les lignes) : les notes
-- et les propositions s'y attachent. Unique dans la recette ; une copie garde les clés de l'originale.
ALTER TABLE recipe_ingredients ADD COLUMN key uuid NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE recipe_variants ADD COLUMN key uuid NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE variant_phases ADD COLUMN key uuid NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE variant_steps ADD COLUMN key uuid NOT NULL DEFAULT gen_random_uuid();

-- Chaque enregistrement qui change la recette : son contenu complet (la recette telle qu'on la publie, clés
-- comprises), qui, quand, et un mot. Revenir à une version, c'est la republier : rien ne se perd.
CREATE TABLE recipe_versions (
    recipe_id   uuid NOT NULL REFERENCES recipes ON DELETE CASCADE,
    number      integer NOT NULL CHECK (number > 0),
    content     jsonb NOT NULL,
    author_id   uuid REFERENCES users ON DELETE SET NULL,
    author_name text NOT NULL,
    message     text NOT NULL DEFAULT '',
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (recipe_id, number)
);

-- L'admin de l'instance se reconnaît à une adresse vérifiée par le fournisseur (Google oui, Amazon non).
ALTER TABLE identities ADD COLUMN email_verified boolean NOT NULL DEFAULT false;
