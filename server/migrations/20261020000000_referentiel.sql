-- M2 : référentiel d'ingrédients adossé à Ciqual (Anses, Licence Ouverte Etalab 2.0).
-- `foods` est rempli au démarrage depuis l'extrait embarqué (server/data/ciqual.tsv), pas par une migration.

-- Version chargée de chaque jeu de données de référence (`ciqual`).
CREATE TABLE reference_data (
    name      text PRIMARY KEY,
    version   text NOT NULL,
    loaded_at timestamptz NOT NULL DEFAULT now()
);

-- Table Ciqual : apports pour 100 g, nuls si non renseignés. Lecture seule.
CREATE TABLE foods (
    code          integer PRIMARY KEY,
    name          text NOT NULL,
    search_key    text NOT NULL,
    group_code    text NOT NULL,
    subgroup_code text NOT NULL,
    energy_kcal   real,
    protein_g     real,
    carbs_g       real,
    sugars_g      real,
    fat_g         real,
    sat_fat_g     real,
    fiber_g       real,
    salt_g        real
);

-- Ingrédients : un nom en base (les traductions viendront dans une table à part) et un état par défaut, la forme
-- d'achat (`fresh` frais ou nature, `frozen` surgelé, `canned` conserve, `dried` sec, `cooked` acheté cuit).
-- Plus de `ciqual_code` : une ligne Ciqual par état, dans `ingredient_foods`.
ALTER TABLE ingredients
    ADD COLUMN name          text,
    ADD COLUMN search_key    text,
    ADD COLUMN default_state text NOT NULL DEFAULT 'fresh'
        CHECK (default_state IN ('fresh', 'frozen', 'canned', 'dried', 'cooked')),
    DROP COLUMN ciqual_code;

UPDATE ingredients SET name = v.name, search_key = v.search_key, default_state = v.state
FROM (VALUES
    ('peanut', 'Arachide', 'arachide', 'dried'),
    ('hazelnut', 'Noisette', 'noisette', 'dried'),
    ('walnut', 'Noix', 'noix', 'dried'),
    ('almond', 'Amande', 'amande', 'dried'),
    ('milk', 'Lait', 'lait', 'fresh'),
    ('cream', 'Crème', 'creme', 'fresh'),
    ('butter', 'Beurre', 'beurre', 'fresh'),
    ('egg', 'Œuf', 'oeuf', 'fresh'),
    ('wheat_flour', 'Farine de blé', 'farine de ble', 'dried'),
    ('pasta', 'Pâtes', 'pates', 'dried'),
    ('rice', 'Riz', 'riz', 'dried'),
    ('shrimp', 'Crevette', 'crevette', 'cooked'),
    ('mussel', 'Moule', 'moule', 'fresh'),
    ('salmon', 'Saumon', 'saumon', 'fresh'),
    ('cod', 'Cabillaud', 'cabillaud', 'fresh'),
    ('tuna', 'Thon', 'thon', 'canned'),
    ('anchovy', 'Anchois', 'anchois', 'canned'),
    ('celery', 'Céleri', 'celeri', 'fresh'),
    ('mustard', 'Moutarde', 'moutarde', 'fresh'),
    ('sesame', 'Sésame', 'sesame', 'dried'),
    ('tofu', 'Tofu', 'tofu', 'fresh'),
    ('mushroom', 'Champignon', 'champignon', 'fresh'),
    ('bell_pepper', 'Poivron', 'poivron', 'fresh'),
    ('onion', 'Oignon', 'oignon', 'fresh'),
    ('garlic', 'Ail', 'ail', 'fresh'),
    ('coriander', 'Coriandre', 'coriandre', 'fresh'),
    ('spinach', 'Épinard', 'epinard', 'fresh'),
    ('cauliflower', 'Chou-fleur', 'chou-fleur', 'fresh'),
    ('broccoli', 'Brocoli', 'brocoli', 'fresh'),
    ('zucchini', 'Courgette', 'courgette', 'fresh'),
    ('eggplant', 'Aubergine', 'aubergine', 'fresh'),
    ('tomato', 'Tomate', 'tomate', 'fresh'),
    ('carrot', 'Carotte', 'carotte', 'fresh'),
    ('potato', 'Pomme de terre', 'pomme de terre', 'fresh'),
    ('beetroot', 'Betterave', 'betterave', 'fresh'),
    ('endive', 'Endive', 'endive', 'fresh'),
    ('leek', 'Poireau', 'poireau', 'fresh'),
    ('olive', 'Olive', 'olive', 'canned'),
    ('lentil', 'Lentilles', 'lentilles', 'dried'),
    ('chickpea', 'Pois chiches', 'pois chiches', 'canned'),
    ('blue_cheese', 'Fromage bleu', 'fromage bleu', 'fresh'),
    ('goat_cheese', 'Fromage de chèvre', 'fromage de chevre', 'fresh'),
    ('chicken', 'Poulet', 'poulet', 'fresh'),
    ('minced_beef', 'Bœuf haché', 'boeuf hache', 'fresh'),
    ('pork', 'Porc', 'porc', 'fresh'),
    ('veal', 'Veau', 'veau', 'fresh'),
    ('lamb', 'Agneau', 'agneau', 'fresh'),
    ('liver', 'Foie', 'foie', 'fresh')
) AS v (code, name, search_key, state)
WHERE ingredients.code = v.code;

ALTER TABLE ingredients
    ALTER COLUMN name SET NOT NULL,
    ALTER COLUMN search_key SET NOT NULL,
    ALTER COLUMN default_state DROP DEFAULT,
    DROP COLUMN name_key;
-- Un nom par ingrédient, aux accents et à la casse près.
CREATE UNIQUE INDEX ingredients_search_key ON ingredients (search_key);

-- Ligne Ciqual de l'ingrédient pour une forme d'achat. Pas de clé étrangère vers `foods`, chargée après les
-- migrations : l'API vérifie le code à l'écriture, et le chargement garde les lignes encore reliées.
CREATE TABLE ingredient_foods (
    ingredient_id uuid NOT NULL REFERENCES ingredients ON DELETE CASCADE,
    state         text NOT NULL CHECK (state IN ('fresh', 'frozen', 'canned', 'dried', 'cooked')),
    food_code     integer NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (ingredient_id, state)
);
CREATE INDEX ingredient_foods_food_code ON ingredient_foods (food_code);

INSERT INTO ingredient_foods (ingredient_id, state, food_code)
SELECT i.id, v.state, v.food_code
FROM (VALUES
    ('peanut', 'dried', 15001),
    ('hazelnut', 'dried', 15004),
    ('walnut', 'dried', 15005),
    ('almond', 'dried', 15000),
    ('milk', 'fresh', 19041),
    ('cream', 'fresh', 19410),
    ('butter', 'fresh', 16400),
    ('egg', 'fresh', 22000),
    ('wheat_flour', 'dried', 9436),
    ('pasta', 'dried', 9810),
    ('rice', 'dried', 9100),
    ('shrimp', 'fresh', 10021),
    ('shrimp', 'frozen', 10038),
    ('shrimp', 'cooked', 10007),
    ('mussel', 'fresh', 10014),
    ('mussel', 'canned', 10028),
    ('salmon', 'fresh', 26036),
    ('cod', 'fresh', 26043),
    ('tuna', 'fresh', 26053),
    ('tuna', 'canned', 26039),
    ('anchovy', 'canned', 26177),
    ('celery', 'fresh', 20023),
    ('mustard', 'fresh', 11013),
    ('sesame', 'dried', 15010),
    ('tofu', 'fresh', 20904),
    ('mushroom', 'fresh', 20056),
    ('mushroom', 'canned', 20079),
    ('bell_pepper', 'fresh', 20041),
    ('onion', 'fresh', 20034),
    ('onion', 'frozen', 20235),
    ('onion', 'dried', 20180),
    ('garlic', 'fresh', 11000),
    ('coriander', 'fresh', 11094),
    ('spinach', 'fresh', 20059),
    ('spinach', 'frozen', 20083),
    ('spinach', 'canned', 20060),
    ('cauliflower', 'fresh', 20016),
    ('cauliflower', 'frozen', 20082),
    ('broccoli', 'fresh', 20057),
    ('broccoli', 'frozen', 20204),
    ('zucchini', 'fresh', 20020),
    ('zucchini', 'frozen', 20230),
    ('eggplant', 'fresh', 20053),
    ('tomato', 'fresh', 20385),
    ('tomato', 'canned', 20048),
    ('carrot', 'fresh', 20009),
    ('carrot', 'frozen', 20208),
    ('carrot', 'canned', 20007),
    ('carrot', 'dried', 20092),
    ('potato', 'fresh', 4008),
    ('beetroot', 'fresh', 20091),
    ('beetroot', 'cooked', 20003),
    ('endive', 'fresh', 20026),
    ('leek', 'fresh', 20039),
    ('leek', 'frozen', 20236),
    ('olive', 'canned', 13033),
    ('lentil', 'canned', 20510),
    ('lentil', 'dried', 20359),
    ('chickpea', 'canned', 20532),
    ('chickpea', 'dried', 20516),
    ('blue_cheese', 'fresh', 12500),
    ('goat_cheese', 'fresh', 12812),
    ('chicken', 'fresh', 36017),
    ('minced_beef', 'fresh', 6254),
    ('pork', 'fresh', 28300),
    ('veal', 'fresh', 6521),
    ('lamb', 'fresh', 21502),
    ('liver', 'fresh', 40106)
) AS v (code, state, food_code)
JOIN ingredients i ON i.code = v.code;
