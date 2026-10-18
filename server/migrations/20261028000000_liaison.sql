-- M4 : liaison de compte Alexa. Chaudron est un serveur OAuth2 minimal : un compte du foyer autorise un client
-- (la skill), qui reçoit un code (5 min, usage unique), puis un jeton de renouvellement (sans rotation) et des jetons
-- d'accès d'une heure. Seuls les hachés sont gardés. Retirer la liaison retire ses jetons d'accès.
CREATE TABLE oauth_grants (
    id              uuid PRIMARY KEY,
    household_id    uuid NOT NULL REFERENCES households ON DELETE CASCADE,
    user_id         uuid NOT NULL REFERENCES users ON DELETE CASCADE,
    client_id       text NOT NULL,
    scopes          text[] NOT NULL,
    redirect_uri    text NOT NULL,
    code_hash       bytea UNIQUE,
    code_expires_at timestamptz,
    refresh_hash    bytea UNIQUE,
    last_used_at    timestamptz,
    created_at      timestamptz NOT NULL DEFAULT now(),
    CHECK ((code_hash IS NULL) = (code_expires_at IS NULL))
);
CREATE INDEX oauth_grants_household_id ON oauth_grants (household_id);

-- Les jetons d'accès d'une liaison expirent ; ceux de Claude, non.
ALTER TABLE api_tokens
    ADD COLUMN expires_at timestamptz,
    ADD COLUMN grant_id uuid REFERENCES oauth_grants ON DELETE CASCADE;
CREATE INDEX api_tokens_grant_id ON api_tokens (grant_id);
