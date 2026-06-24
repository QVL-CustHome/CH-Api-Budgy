CREATE SCHEMA IF NOT EXISTS budgy;

CREATE TABLE budgy.bank_credential (
    id           UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id     TEXT         NOT NULL,
    access_token BYTEA        NOT NULL,
    key_version  SMALLINT     NOT NULL DEFAULT 1,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX bank_credential_owner ON budgy.bank_credential (owner_id);
