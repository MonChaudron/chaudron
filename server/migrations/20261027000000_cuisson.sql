-- M4 : la cuisson en cours, partagée entre l'Echo Show et le téléphone. Une par foyer, position absolue.
-- Ni créneau ni variante par identifiant : republier un menu ou une recette les recrée. Le créneau se retrouve par
-- la date, le repas et la recette ; la variante par son nom.
CREATE TABLE cooking_sessions (
    household_id  uuid PRIMARY KEY REFERENCES households ON DELETE CASCADE,
    recipe_id     uuid NOT NULL REFERENCES recipes ON DELETE CASCADE,
    variant_label text NOT NULL,
    date          date,
    meal          text CHECK (meal IN ('lunch', 'dinner')),
    mode          text NOT NULL CHECK (mode IN ('fast', 'guided')),
    phase         smallint NOT NULL CHECK (phase >= 0),
    step          smallint CHECK (step >= 0),
    started_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    CHECK ((date IS NULL) = (meal IS NULL))
);

-- La skill Alexa avance dans la cuisson.
ALTER TABLE api_tokens DROP CONSTRAINT api_tokens_scopes_check;
ALTER TABLE api_tokens ADD CONSTRAINT api_tokens_scopes_check
    CHECK (scopes <@ ARRAY['read', 'recipes:write', 'menus:write', 'cooking:write'] AND cardinality(scopes) > 0);
