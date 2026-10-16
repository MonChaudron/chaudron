-- M1 : comptes, sessions, foyers, membres (barème, présence, exclusions), équipements, ingrédients.
-- Identifiants en UUID v7, générés par le serveur (PostgreSQL 17 n'a pas uuidv7()).

-- Comptes

CREATE TABLE users (
    id           uuid PRIMARY KEY,
    display_name text NOT NULL,
    email        text,
    locale       text NOT NULL DEFAULT 'fr-FR',
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);

-- Un utilisateur peut lier Amazon et Google. `fake` : fournisseur factice de dev et de test.
CREATE TABLE identities (
    provider   text NOT NULL CHECK (provider IN ('amazon', 'google', 'fake')),
    subject    text NOT NULL,
    user_id    uuid NOT NULL REFERENCES users ON DELETE CASCADE,
    email      text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (provider, subject)
);
CREATE INDEX identities_user_id ON identities (user_id);

-- Sessions web : seul le hash SHA-256 du jeton du cookie est stocké. Expiration glissante.
CREATE TABLE sessions (
    token_hash   bytea PRIMARY KEY,
    user_id      uuid NOT NULL REFERENCES users ON DELETE CASCADE,
    expires_at   timestamptz NOT NULL,
    last_seen_at timestamptz NOT NULL DEFAULT now(),
    created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX sessions_user_id ON sessions (user_id);

-- Foyers

CREATE TABLE households (
    id         uuid PRIMARY KEY,
    name       text NOT NULL,
    timezone   text NOT NULL DEFAULT 'Europe/Paris',
    locale     text NOT NULL DEFAULT 'fr-FR',
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- Qui peut voir et modifier le foyer. `owner` gère en plus les comptes (invitations, rôles).
CREATE TABLE household_users (
    household_id uuid NOT NULL REFERENCES households ON DELETE CASCADE,
    user_id      uuid NOT NULL REFERENCES users ON DELETE CASCADE,
    role         text NOT NULL CHECK (role IN ('owner', 'member')),
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (household_id, user_id)
);
CREATE INDEX household_users_user_id ON household_users (user_id);

-- Lien d'invitation à usage unique : seul le hash du jeton est stocké.
CREATE TABLE household_invites (
    id           uuid PRIMARY KEY,
    household_id uuid NOT NULL REFERENCES households ON DELETE CASCADE,
    token_hash   bytea NOT NULL UNIQUE,
    created_by   uuid REFERENCES users ON DELETE SET NULL,
    expires_at   timestamptz NOT NULL,
    used_by      uuid REFERENCES users ON DELETE SET NULL,
    used_at      timestamptz,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX household_invites_household_id ON household_invites (household_id);

-- Référentiels

CREATE TABLE ingredients (
    id           uuid PRIMARY KEY,
    code         text NOT NULL UNIQUE,
    name_key     text NOT NULL,
    aisle        text NOT NULL CHECK (aisle IN ('fresh', 'produce', 'butcher', 'fish', 'dairy', 'frozen', 'grocery', 'bakery', 'other')),
    default_unit text NOT NULL CHECK (default_unit IN ('g', 'ml', 'piece', 'tbsp', 'tsp', 'pinch')),
    ciqual_code  text,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE equipment (
    code       text PRIMARY KEY,
    name_key   text NOT NULL,
    rules      jsonb NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE household_equipment (
    household_id   uuid NOT NULL REFERENCES households ON DELETE CASCADE,
    equipment_code text NOT NULL REFERENCES equipment,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (household_id, equipment_code)
);

-- Qui mange, et combien

-- Un membre n'a pas forcément de compte (les enfants) ; un compte est au plus un membre par foyer.
CREATE TABLE members (
    id           uuid PRIMARY KEY,
    household_id uuid NOT NULL REFERENCES households ON DELETE CASCADE,
    name         text NOT NULL,
    kind         text NOT NULL CHECK (kind IN ('adult', 'teen', 'child')),
    diet         text NOT NULL DEFAULT 'omnivore' CHECK (diet IN ('omnivore', 'pescatarian', 'vegetarian', 'vegan')),
    user_id      uuid REFERENCES users ON DELETE SET NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    UNIQUE (household_id, user_id)
);
CREATE INDEX members_household_id ON members (household_id);

-- Barème : multiplicateur par rôle d'ingrédient, appliqué à une portion standard.
CREATE TABLE member_portions (
    member_id  uuid NOT NULL REFERENCES members ON DELETE CASCADE,
    role       text NOT NULL CHECK (role IN ('protein', 'starch', 'veg', 'fat', 'other')),
    factor     real NOT NULL CHECK (factor >= 0 AND factor <= 5),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (member_id, role)
);

-- `allergy` : filtre dur ; `dislike` : à éviter.
CREATE TABLE member_exclusions (
    member_id     uuid NOT NULL REFERENCES members ON DELETE CASCADE,
    ingredient_id uuid NOT NULL REFERENCES ingredients,
    kind          text NOT NULL CHECK (kind IN ('allergy', 'dislike')),
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (member_id, ingredient_id)
);

-- weekday : 1 = lundi … 7 = dimanche (ISO 8601).
CREATE TABLE member_presence (
    member_id  uuid NOT NULL REFERENCES members ON DELETE CASCADE,
    weekday    smallint NOT NULL CHECK (weekday BETWEEN 1 AND 7),
    meal       text NOT NULL CHECK (meal IN ('lunch', 'dinner')),
    present    boolean NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (member_id, weekday, meal)
);

-- Catalogue d'équipements. Règles du Cookeo : fiche « Le Cookeo, et ce qu'il ne peut pas avaler ».
INSERT INTO equipment (code, name_key, rules) VALUES
    ('thermomix_tm6',   'equipment.thermomix_tm6',   '{}'),
    ('cookeo_6l',       'equipment.cookeo_6l',       '{"max_fill_l": 4, "max_fill_swelling_l": 3, "min_liquid_ml": 250}'),
    ('pressure_cooker', 'equipment.pressure_cooker', '{}'),
    ('oven',            'equipment.oven',            '{}'),
    ('hob',             'equipment.hob',             '{}'),
    ('microwave',       'equipment.microwave',       '{}'),
    ('airfryer',        'equipment.airfryer',        '{}');

-- Premiers ingrédients : allergènes et refus courants, pour les exclusions des membres. Le référentiel grandit en M2.
INSERT INTO ingredients (id, code, name_key, aisle, default_unit) VALUES
    ('01a14728-8400-783d-83dd-77139ed52d8b', 'peanut', 'ingredient.peanut', 'grocery', 'g'),
    ('01a14728-8401-76ed-ae5e-e60895306530', 'hazelnut', 'ingredient.hazelnut', 'grocery', 'g'),
    ('01a14728-8402-7498-a306-99be85728371', 'walnut', 'ingredient.walnut', 'grocery', 'g'),
    ('01a14728-8403-7168-8355-e76cfd1fe8ef', 'almond', 'ingredient.almond', 'grocery', 'g'),
    ('01a14728-8404-73bf-aed0-8de67a6b1251', 'milk', 'ingredient.milk', 'dairy', 'ml'),
    ('01a14728-8405-7a16-8ecd-87a68c7134eb', 'cream', 'ingredient.cream', 'dairy', 'ml'),
    ('01a14728-8406-7e40-8fcf-94cb1459c6c0', 'butter', 'ingredient.butter', 'dairy', 'g'),
    ('01a14728-8407-78cb-af87-5161b1eff331', 'egg', 'ingredient.egg', 'fresh', 'piece'),
    ('01a14728-8408-7fcf-b9f2-22fa8fb1c78f', 'wheat_flour', 'ingredient.wheat_flour', 'grocery', 'g'),
    ('01a14728-8409-7b12-9d0e-ac8686f0d2a5', 'pasta', 'ingredient.pasta', 'grocery', 'g'),
    ('01a14728-840a-7858-9b1a-f6bf41338f82', 'rice', 'ingredient.rice', 'grocery', 'g'),
    ('01a14728-840b-7094-ab79-d1ea98af286a', 'shrimp', 'ingredient.shrimp', 'fish', 'g'),
    ('01a14728-840c-79c7-81ab-2699ff01f60c', 'mussel', 'ingredient.mussel', 'fish', 'g'),
    ('01a14728-840d-77f1-aecf-f26132aeef40', 'salmon', 'ingredient.salmon', 'fish', 'g'),
    ('01a14728-840e-7ed2-afe0-31558b20c414', 'cod', 'ingredient.cod', 'fish', 'g'),
    ('01a14728-840f-712a-9568-a03472dce082', 'tuna', 'ingredient.tuna', 'grocery', 'g'),
    ('01a14728-8410-74a6-a96d-f633f68f101c', 'anchovy', 'ingredient.anchovy', 'grocery', 'g'),
    ('01a14728-8411-7fbb-80ba-e9ec8f56b02b', 'celery', 'ingredient.celery', 'produce', 'g'),
    ('01a14728-8412-757c-9046-a7de3e1554e3', 'mustard', 'ingredient.mustard', 'grocery', 'tsp'),
    ('01a14728-8413-7c33-8869-86c876190f4a', 'sesame', 'ingredient.sesame', 'grocery', 'g'),
    ('01a14728-8414-7f78-9b11-d70d945349e9', 'tofu', 'ingredient.tofu', 'fresh', 'g'),
    ('01a14728-8415-7f97-8524-c17cbf721f81', 'mushroom', 'ingredient.mushroom', 'produce', 'g'),
    ('01a14728-8416-78c4-be59-3c1984aab954', 'bell_pepper', 'ingredient.bell_pepper', 'produce', 'piece'),
    ('01a14728-8417-75c0-b853-a48e05b5b6c2', 'onion', 'ingredient.onion', 'produce', 'piece'),
    ('01a14728-8418-7da2-9a16-5a98b385f0c9', 'garlic', 'ingredient.garlic', 'produce', 'piece'),
    ('01a14728-8419-7b82-8da9-54ada966d78f', 'coriander', 'ingredient.coriander', 'produce', 'g'),
    ('01a14728-841a-7340-873d-b24b3cbd0703', 'spinach', 'ingredient.spinach', 'produce', 'g'),
    ('01a14728-841b-70b8-96c7-c5dea52cbd59', 'cauliflower', 'ingredient.cauliflower', 'produce', 'g'),
    ('01a14728-841c-768c-8263-2c464d1cdf01', 'broccoli', 'ingredient.broccoli', 'produce', 'g'),
    ('01a14728-841d-7f97-8313-9b825190d821', 'zucchini', 'ingredient.zucchini', 'produce', 'piece'),
    ('01a14728-841e-73c6-81a6-e89ce3df3d37', 'eggplant', 'ingredient.eggplant', 'produce', 'piece'),
    ('01a14728-841f-7d40-a9ce-2610ce0aa088', 'tomato', 'ingredient.tomato', 'produce', 'piece'),
    ('01a14728-8420-79de-8e69-f627f96dd417', 'carrot', 'ingredient.carrot', 'produce', 'g'),
    ('01a14728-8421-7926-aaa1-bb1f6a16d0bf', 'potato', 'ingredient.potato', 'produce', 'g'),
    ('01a14728-8422-7c26-82ee-5c94ed8cebd2', 'beetroot', 'ingredient.beetroot', 'produce', 'g'),
    ('01a14728-8423-7e6e-9ef8-2f025ccc505a', 'endive', 'ingredient.endive', 'produce', 'piece'),
    ('01a14728-8424-77c8-982b-88bd73864064', 'leek', 'ingredient.leek', 'produce', 'piece'),
    ('01a14728-8425-725a-bd5d-1522295b1d91', 'olive', 'ingredient.olive', 'grocery', 'g'),
    ('01a14728-8426-7d52-93f1-f7119f51b557', 'lentil', 'ingredient.lentil', 'grocery', 'g'),
    ('01a14728-8427-796b-9b28-3f3c94a60933', 'chickpea', 'ingredient.chickpea', 'grocery', 'g'),
    ('01a14728-8428-7c46-b177-f59cb1db6b09', 'blue_cheese', 'ingredient.blue_cheese', 'dairy', 'g'),
    ('01a14728-8429-7cd4-91b6-c58cf5079e3e', 'goat_cheese', 'ingredient.goat_cheese', 'dairy', 'g'),
    ('01a14728-842a-7c92-affa-64d58f486abd', 'chicken', 'ingredient.chicken', 'butcher', 'g'),
    ('01a14728-842b-7ed6-833b-2a08df0970c0', 'minced_beef', 'ingredient.minced_beef', 'butcher', 'g'),
    ('01a14728-842c-7f56-b41b-d2d0e4e577f4', 'pork', 'ingredient.pork', 'butcher', 'g'),
    ('01a14728-842d-79b3-af36-6e534b4b5164', 'veal', 'ingredient.veal', 'butcher', 'g'),
    ('01a14728-842e-7130-83bc-a075fe7e82b8', 'lamb', 'ingredient.lamb', 'butcher', 'g'),
    ('01a14728-842f-78fb-b953-7456e1c00b68', 'liver', 'ingredient.liver', 'butcher', 'g');
