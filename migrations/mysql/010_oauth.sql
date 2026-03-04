-- migrations/mysql/010_oauth.sql
CREATE TABLE oauth_identities (
    id           BIGINT       NOT NULL AUTO_INCREMENT PRIMARY KEY,
    user_id      BIGINT       NOT NULL,
    provider     VARCHAR(32)  NOT NULL,
    provider_uid VARCHAR(255) NOT NULL,
    email        VARCHAR(255),
    display_name VARCHAR(255),
    status       VARCHAR(16)  NOT NULL DEFAULT 'pending',
    rejected_at  VARCHAR(64),
    created_at   VARCHAR(32)  NOT NULL,
    UNIQUE KEY uq_provider_uid (provider, provider_uid),
    CONSTRAINT fk_oauth_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_oauth_identities_user_id ON oauth_identities(user_id);
CREATE INDEX idx_oauth_identities_status  ON oauth_identities(status);
