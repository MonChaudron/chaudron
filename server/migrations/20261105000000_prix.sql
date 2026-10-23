-- Prix estimés (design-prix.md) : chaque ingrédient, par forme d'achat, est rattaché à des sources publiques
-- (Open Prices, INSEE) ; une tâche mensuelle du serveur en tire un prix unitaire moyen. Pas de prix saisi à la main :
-- seul l'admin d'instance corrige un rattachement.

-- Poids d'une pièce (un oignon, une gousse d'ail) et densité (g/ml), pour passer des unités de recette à celles du
-- prix ; nuls quand ils ne sont pas connus.
ALTER TABLE ingredients
    ADD COLUMN piece_g real CHECK (piece_g > 0),
    ADD COLUMN density real CHECK (density > 0);

-- `off_category` : relevés de produits en vrac (Open Prices `category_tag`) ; `off_product_category` : produits à
-- code-barres de cette catégorie Open Food Facts ; `insee_series` : idbank d'un prix moyen de vente de détail, en
-- `insee_per` ; `insee_index` : idbank de l'indice des prix de son poste, pour actualiser un prix ancien.
CREATE TABLE ingredient_price_links (
    ingredient_id        uuid NOT NULL REFERENCES ingredients (id) ON DELETE CASCADE,
    state                text NOT NULL CHECK (state IN ('fresh', 'frozen', 'canned', 'dried', 'cooked')),
    off_category         text,
    off_product_category text,
    insee_series         text,
    insee_per            text CHECK (insee_per IN ('kg', 'l', 'piece')),
    insee_index          text,
    updated_at           timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (ingredient_id, state)
);

-- Prix d'une unité de référence (`per`), en centimes. `observed_on` : date médiane des relevés retenus ;
-- `insee_cents` : le prix moyen INSEE du dernier mois publié, quand il y en a un ; `flag` : `divergent` quand les
-- deux sources s'écartent trop (le rattachement est sans doute faux).
CREATE TABLE ingredient_prices (
    ingredient_id uuid NOT NULL REFERENCES ingredients (id) ON DELETE CASCADE,
    state         text NOT NULL CHECK (state IN ('fresh', 'frozen', 'canned', 'dried', 'cooked')),
    cents         integer NOT NULL CHECK (cents > 0),
    per           text NOT NULL CHECK (per IN ('kg', 'l', 'piece')),
    source        text NOT NULL CHECK (source IN ('open_prices', 'insee', 'indexed')),
    observations  integer NOT NULL,
    observed_on   date NOT NULL,
    insee_cents   integer,
    flag          text CHECK (flag IN ('divergent')),
    computed_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (ingredient_id, state)
);

-- Passages de la mise à jour : le prochain a lieu quand le dernier réussi a plus de 30 jours.
CREATE TABLE price_runs (
    id          uuid PRIMARY KEY,
    started_at  timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz,
    ok          boolean NOT NULL DEFAULT false,
    priced      integer NOT NULL DEFAULT 0,
    message     text NOT NULL DEFAULT ''
);

-- Plafond de budget d'une semaine de menu, facultatif.
ALTER TABLE households ADD COLUMN weekly_budget_cents integer CHECK (weekly_budget_cents BETWEEN 100 AND 1000000);
