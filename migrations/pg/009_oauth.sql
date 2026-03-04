-- migrations/pg/009_oauth.sql
CREATE TABLE oauth_identities (
    id           BIGSERIAL PRIMARY KEY,
    user_id      BIGINT  NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider     TEXT    NOT NULL,
    provider_uid TEXT    NOT NULL,
    email        TEXT,
    display_name TEXT,
    status       TEXT    NOT NULL DEFAULT 'pending',
    rejected_at  TEXT,
    created_at   TEXT    NOT NULL,
    UNIQUE(provider, provider_uid)
);

CREATE INDEX idx_oauth_identities_user_id ON oauth_identities(user_id);
CREATE INDEX idx_oauth_identities_status  ON oauth_identities(status);
