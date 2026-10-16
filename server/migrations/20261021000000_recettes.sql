-- M2 : recettes du foyer, variantes par équipement (phases du mode rapide, gestes du pas à pas), jetons d'API.

-- Jetons d'API par foyer (Claude, scripts) : seul le hash SHA-256 est stocké.
CREATE TABLE api_tokens (
    id           uuid PRIMARY KEY,
    household_id uuid NOT NULL REFERENCES households ON DELETE CASCADE,
    name         text NOT NULL,
    token_hash   bytea NOT NULL UNIQUE,
    scopes       text[] NOT NULL CHECK (scopes <@ ARRAY['read', 'recipes:write', 'menus:write'] AND cardinality(scopes) > 0),
    created_by   uuid REFERENCES users ON DELETE SET NULL,
    last_used_at timestamptz,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX api_tokens_household_id ON api_tokens (household_id);

-- Une recette appartient à un foyer (le catalogue commun, `household_id` nul, viendra en V2).
CREATE TABLE recipes (
    id            uuid PRIMARY KEY,
    household_id  uuid NOT NULL REFERENCES households ON DELETE CASCADE,
    slug          text NOT NULL CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$' AND length(slug) <= 80),
    title         text NOT NULL,
    search_key    text NOT NULL,
    description   text NOT NULL DEFAULT '',
    tags          text[] NOT NULL DEFAULT '{}',
    season_months smallint[] NOT NULL DEFAULT '{}',
    source        text,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (household_id, slug)
);

-- Pour une portion standard, quantité telle qu'achetée (nulle : « au goût »). `per_batch` : ne suit pas le nombre de
-- portions (un citron, un cube de bouillon).
CREATE TABLE recipe_ingredients (
    recipe_id     uuid NOT NULL REFERENCES recipes ON DELETE CASCADE,
    position      smallint NOT NULL,
    ingredient_id uuid NOT NULL REFERENCES ingredients,
    state         text NOT NULL CHECK (state IN ('fresh', 'frozen', 'canned', 'dried', 'cooked')),
    qty           real CHECK (qty > 0),
    unit          text NOT NULL CHECK (unit IN ('g', 'ml', 'piece', 'tbsp', 'tsp', 'pinch')),
    role          text NOT NULL CHECK (role IN ('protein', 'starch', 'veg', 'fat', 'other')),
    optional      boolean NOT NULL DEFAULT false,
    per_batch     boolean NOT NULL DEFAULT false,
    note          text,
    PRIMARY KEY (recipe_id, position)
);
CREATE INDEX recipe_ingredients_ingredient_id ON recipe_ingredients (ingredient_id);

-- Une variante = une manière de cuisiner, avec les équipements qu'elle demande.
CREATE TABLE recipe_variants (
    id              uuid PRIMARY KEY,
    recipe_id       uuid NOT NULL REFERENCES recipes ON DELETE CASCADE,
    position        smallint NOT NULL,
    label           text NOT NULL,
    equipment_codes text[] NOT NULL DEFAULT '{}',
    active_min      smallint NOT NULL CHECK (active_min >= 0),
    total_min       smallint NOT NULL CHECK (total_min >= active_min),
    vessels         smallint NOT NULL DEFAULT 1 CHECK (vessels >= 0),
    is_default      boolean NOT NULL,
    UNIQUE (recipe_id, position)
);
CREATE UNIQUE INDEX recipe_variants_one_default ON recipe_variants (recipe_id) WHERE is_default;

-- Une étape du mode rapide. `day_offset` -1 : la veille (pâte à pizza).
CREATE TABLE variant_phases (
    id          uuid PRIMARY KEY,
    variant_id  uuid NOT NULL REFERENCES recipe_variants ON DELETE CASCADE,
    position    smallint NOT NULL,
    title       text NOT NULL,
    text        text NOT NULL,
    spoken_text text,
    timer_s     integer CHECK (timer_s > 0),
    day_offset  smallint NOT NULL DEFAULT 0 CHECK (day_offset BETWEEN -7 AND 0),
    UNIQUE (variant_id, position)
);

-- Un geste du mode pas à pas (facultatif). `params` : vitesse, température, programme, pour les futurs connecteurs.
CREATE TABLE variant_steps (
    id             uuid PRIMARY KEY,
    phase_id       uuid NOT NULL REFERENCES variant_phases ON DELETE CASCADE,
    position       smallint NOT NULL,
    text           text NOT NULL,
    spoken_text    text,
    timer_s        integer CHECK (timer_s > 0),
    video_url      text CHECK (video_url LIKE 'https://%'),
    equipment_code text REFERENCES equipment,
    params         jsonb NOT NULL DEFAULT '{}',
    UNIQUE (phase_id, position)
);

-- Le blender des gaspachos et des veloutés.
INSERT INTO equipment (code, name_key) VALUES ('blender', 'equipment.blender');
