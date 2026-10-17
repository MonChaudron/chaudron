-- M3 : menus du foyer. Un menu = une période qui commence à son premier repas (`week_start`, n'importe quel jour) ;
-- un créneau = un plat (deux plats le même soir = deux créneaux, rangés par `position`).

CREATE TABLE menus (
    id           uuid PRIMARY KEY,
    household_id uuid NOT NULL REFERENCES households ON DELETE CASCADE,
    week_start   date NOT NULL,
    title        text NOT NULL DEFAULT '',
    notes        text NOT NULL DEFAULT '',
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    UNIQUE (household_id, week_start)
);

-- Portions : mangeurs du repas, plus les boîtes du lendemain midi si `covers_next_lunch`. Par défaut, les membres
-- présents ce jour-là (`portions_date`, ou la date du créneau) ; `diners` et `next_lunch_covers` les remplacent en
-- nombre. `variant_label` : la variante par son nom, qui survit à la republication de la recette ; nul = par défaut.
-- Supprimer une recette prévue dans un menu est refusé par l'API (409) ; la cascade sert à supprimer le foyer.
CREATE TABLE menu_slots (
    id                uuid PRIMARY KEY,
    menu_id           uuid NOT NULL REFERENCES menus ON DELETE CASCADE,
    date              date NOT NULL,
    meal              text NOT NULL CHECK (meal IN ('lunch', 'dinner')),
    position          smallint NOT NULL,
    recipe_id         uuid NOT NULL REFERENCES recipes ON DELETE CASCADE,
    variant_label     text,
    covers_next_lunch boolean NOT NULL DEFAULT false,
    diners            smallint CHECK (diners >= 0),
    next_lunch_covers smallint CHECK (next_lunch_covers >= 0),
    portions_date     date,
    note              text,
    UNIQUE (menu_id, date, meal, position) DEFERRABLE INITIALLY DEFERRED
);
CREATE INDEX menu_slots_recipe_id ON menu_slots (recipe_id);
CREATE INDEX menu_slots_date ON menu_slots (date);

-- Appareils qu'on ne partage pas entre deux plats du même repas (le four et la plaque se partagent).
UPDATE equipment SET rules = rules || '{"exclusive": true}'
WHERE code IN ('cookeo_6l', 'thermomix_tm6', 'pressure_cooker', 'airfryer', 'blender');
