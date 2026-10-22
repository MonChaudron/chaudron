-- M5 : retours de la communauté. Un foyer appuie un retour publié (« +1 », « pareil chez nous ») : un appui par foyer.
CREATE TABLE feedback_support (
    feedback_id  uuid NOT NULL REFERENCES recipe_feedback ON DELETE CASCADE,
    household_id uuid NOT NULL REFERENCES households ON DELETE CASCADE,
    created_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (feedback_id, household_id)
);
