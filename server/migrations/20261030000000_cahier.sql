-- M5 : retours sur une recette. Une seule table pour le cahier du foyer (notes privées, `household`) et les retours
-- de la communauté (`community`, validés par l'admin) : sur une recette du foyer, une note et un commentaire du foyer
-- ne diffèrent en rien. Un retour vise la recette entière, ou un élément par sa clé (phase, geste, ligne).

CREATE TABLE recipe_feedback (
    id             uuid PRIMARY KEY,
    recipe_id      uuid NOT NULL REFERENCES recipes ON DELETE CASCADE,
    -- Foyer de l'auteur. Ses notes privées et ses retours en attente partent avec lui (l'API les supprime) ; ses
    -- retours publiés restent, signés « un foyer ».
    household_id   uuid REFERENCES households ON DELETE SET NULL,
    author_id      uuid REFERENCES users ON DELETE SET NULL,
    author_name    text NOT NULL,
    parent_id      uuid REFERENCES recipe_feedback ON DELETE CASCADE,
    -- Clé de la phase, du geste ou de la ligne ; nulle : la recette entière.
    element_key    uuid,
    visibility     text NOT NULL CHECK (visibility IN ('household', 'community')),
    kind           text NOT NULL CHECK (kind IN ('comment', 'proposal', 'tried')),
    status         text NOT NULL CHECK (status IN ('open', 'closed', 'pending', 'published', 'applied', 'refused')),
    body           text NOT NULL,
    -- Par champ : `{"timer_s": {"from": 720, "to": 840}}`. Sur une note du foyer, affiché à côté du texte de l'étape
    -- sans toucher la recette ; sur une proposition, ce qu'appliquer changerait.
    changes        jsonb,
    -- Version de la recette sur laquelle le retour a été écrit.
    base_version   integer NOT NULL,
    -- Variante tentée : ingrédient remplacé, remplaçant, verdict.
    tried          jsonb,
    review_message text,
    reviewed_by    uuid REFERENCES users ON DELETE SET NULL,
    reviewed_at    timestamptz,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    CHECK ((visibility = 'household') = (status IN ('open', 'closed')))
);
CREATE INDEX recipe_feedback_recipe_id ON recipe_feedback (recipe_id);
CREATE INDEX recipe_feedback_household_id ON recipe_feedback (household_id);

-- « Comment c'était ? » : une note par foyer et par repas, comptée par recette.
CREATE TABLE meal_ratings (
    household_id uuid NOT NULL REFERENCES households ON DELETE CASCADE,
    recipe_id    uuid NOT NULL REFERENCES recipes ON DELETE CASCADE,
    date         date NOT NULL,
    meal         text NOT NULL CHECK (meal IN ('lunch', 'dinner')),
    rating       text NOT NULL CHECK (rating IN ('top', 'bof', 'flop')),
    user_id      uuid REFERENCES users ON DELETE SET NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (household_id, recipe_id, date, meal)
);
CREATE INDEX meal_ratings_recipe_id ON meal_ratings (recipe_id);
