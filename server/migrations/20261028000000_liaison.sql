-- M4 : liaison de compte Alexa. Chaudron est un serveur OAuth2 minimal : un compte du foyer autorise un client
-- (la skill), qui reçoit un code (5 min, usage unique), puis un jeton de renouvellement (sans rotation) et des jetons
-- d'accès d'une heure. Seuls les hachés sont gardés. Retirer la liaison retire ses jetons d'accès ; la liaison suit
-- le compte qui l'a faite : s'il quitte le foyer (ou en est retiré), elle part avec lui.
CREATE TABLE oauth_grants (
    id              uuid PRIMARY KEY,
    household_id    uuid NOT NULL,
    user_id         uuid NOT NULL,
    client_id       text NOT NULL,
    scopes          text[] NOT NULL,
    redirect_uri    text NOT NULL,
    code_hash       bytea UNIQUE,
    code_expires_at timestamptz,
    refresh_hash    bytea UNIQUE,
    last_used_at    timestamptz,
    created_at      timestamptz NOT NULL DEFAULT now(),
    CHECK ((code_hash IS NULL) = (code_expires_at IS NULL)),
    FOREIGN KEY (household_id, user_id) REFERENCES household_users ON DELETE CASCADE
);
CREATE INDEX oauth_grants_household_id ON oauth_grants (household_id);

-- Les jetons d'accès d'une liaison expirent ; ceux de Claude, non.
ALTER TABLE api_tokens
    ADD COLUMN expires_at timestamptz,
    ADD COLUMN grant_id uuid REFERENCES oauth_grants ON DELETE CASCADE;
CREATE INDEX api_tokens_grant_id ON api_tokens (grant_id);
