-- M5 : proposer une recette du foyer à la communauté (l'admin de l'instance accepte ou refuse), copier une recette de
-- la communauté chez soi, et prévenir les auteurs.

-- Une copie garde le lien avec l'originale et la version de l'originale qu'elle suit (« d'après Hachis parmentier,
-- v3 ») : au-delà, l'originale a changé depuis.
ALTER TABLE recipes ADD COLUMN copied_from uuid REFERENCES recipes ON DELETE SET NULL;
ALTER TABLE recipes ADD COLUMN copied_version integer;

-- Une recette du foyer proposée à la communauté. Acceptée, elle devient la recette de la communauté (même identifiant :
-- les menus et la cuisson du foyer ne bougent pas).
CREATE TABLE recipe_submissions (
    id             uuid PRIMARY KEY,
    recipe_id      uuid NOT NULL REFERENCES recipes ON DELETE CASCADE,
    household_id   uuid REFERENCES households ON DELETE CASCADE,
    author_id      uuid REFERENCES users ON DELETE SET NULL,
    author_name    text NOT NULL,
    message        text NOT NULL DEFAULT '',
    status         text NOT NULL CHECK (status IN ('pending', 'accepted', 'refused')),
    review_message text,
    reviewed_by    uuid REFERENCES users ON DELETE SET NULL,
    reviewed_at    timestamptz,
    created_at     timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX recipe_submissions_one_pending ON recipe_submissions (recipe_id) WHERE status = 'pending';

-- Ce qu'on dit à l'auteur d'une proposition ou d'un retour : dans l'appli (liste et badge), pas d'e-mail.
CREATE TABLE notifications (
    id         uuid PRIMARY KEY,
    user_id    uuid NOT NULL REFERENCES users ON DELETE CASCADE,
    kind       text NOT NULL,
    recipe_id  uuid REFERENCES recipes ON DELETE CASCADE,
    text       text NOT NULL,
    read_at    timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX notifications_user_id ON notifications (user_id, created_at DESC);
