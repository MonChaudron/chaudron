-- M3 : liste de courses d'un menu. La liste est calculée à la lecture depuis les créneaux ; la base ne garde que
-- l'état des articles (case cochée, par clé ingrédient + forme + unité) et les ajouts à la main.

CREATE TABLE shopping_items (
    id           uuid PRIMARY KEY,
    menu_id      uuid NOT NULL REFERENCES menus ON DELETE CASCADE,
    -- `{ingredient_id}:{état}:{unité}` pour un article calculé, `manual:{uuid}` pour un ajout à la main.
    key          text NOT NULL,
    manual       boolean NOT NULL,
    label        text,
    qty          real CHECK (qty > 0),
    unit         text CHECK (unit IN ('g', 'ml', 'piece', 'tbsp', 'tsp', 'pinch')),
    aisle        text CHECK (aisle IN ('fresh', 'produce', 'butcher', 'fish', 'dairy', 'frozen', 'grocery', 'bakery', 'other')),
    block        text NOT NULL DEFAULT 'recipes' CHECK (block IN ('recipes', 'breakfast_desserts', 'special')),
    checked      boolean NOT NULL DEFAULT false,
    checked_by   uuid REFERENCES users ON DELETE SET NULL,
    -- Heure du geste sur le téléphone (jour d'achat, pour la fraîcheur).
    checked_at   timestamptz,
    -- Quantité au moment de cocher : signale une hausse quand le menu change ensuite.
    qty_at_check real,
    removed      boolean NOT NULL DEFAULT false,
    -- Heure de la dernière opération appliquée : une opération plus ancienne est ignorée (dernière écriture gagne).
    op_at        timestamptz NOT NULL,
    UNIQUE (menu_id, key)
);

-- Journal des opérations : idempotence (`op_id`) et curseur des clients (`seq`).
CREATE TABLE shopping_ops (
    seq          bigserial PRIMARY KEY,
    household_id uuid NOT NULL REFERENCES households ON DELETE CASCADE,
    menu_id      uuid NOT NULL REFERENCES menus ON DELETE CASCADE,
    op_id        uuid NOT NULL UNIQUE,
    at           timestamptz NOT NULL,
    user_id      uuid REFERENCES users ON DELETE SET NULL,
    kind         text NOT NULL CHECK (kind IN ('check', 'uncheck', 'add', 'edit', 'remove')),
    key          text NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX shopping_ops_household_seq ON shopping_ops (household_id, seq);
