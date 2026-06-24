CREATE TABLE budgy.consent (
    id           UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id     TEXT         NOT NULL,
    external_ref BYTEA        NOT NULL,
    status       TEXT         NOT NULL,
    expires_at   TIMESTAMPTZ,
    key_version  SMALLINT     NOT NULL DEFAULT 1,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX consent_owner ON budgy.consent (owner_id);

CREATE TABLE budgy.bank_account (
    id                  UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id            TEXT         NOT NULL,
    consent_id          UUID         NOT NULL REFERENCES budgy.consent (id) ON DELETE CASCADE,
    external_account_id BYTEA        NOT NULL,
    iban_encrypted      BYTEA        NOT NULL,
    iban_masked         TEXT         NOT NULL,
    currency            TEXT         NOT NULL,
    next_sync_at        TIMESTAMPTZ,
    sync_count_today    INTEGER      NOT NULL DEFAULT 0,
    key_version         SMALLINT     NOT NULL DEFAULT 1,
    created_at          TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX bank_account_owner ON budgy.bank_account (owner_id);
CREATE INDEX bank_account_consent ON budgy.bank_account (consent_id);

CREATE TABLE budgy.balance (
    id              UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    bank_account_id UUID         NOT NULL REFERENCES budgy.bank_account (id) ON DELETE CASCADE,
    balance_type    TEXT         NOT NULL,
    amount_cents    BYTEA        NOT NULL,
    currency        TEXT         NOT NULL,
    reference_date  TIMESTAMPTZ  NOT NULL,
    key_version     SMALLINT     NOT NULL DEFAULT 1,
    created_at      TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX balance_bank_account ON budgy.balance (bank_account_id);

CREATE TABLE budgy.bank_transaction (
    id                      UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    bank_account_id         UUID         NOT NULL REFERENCES budgy.bank_account (id) ON DELETE CASCADE,
    external_transaction_id BYTEA        NOT NULL,
    dedup_key               TEXT         NOT NULL,
    status                  TEXT         NOT NULL,
    label                   BYTEA        NOT NULL,
    amount_cents            BYTEA        NOT NULL,
    currency                TEXT         NOT NULL,
    booking_date            DATE,
    value_date              DATE,
    key_version             SMALLINT     NOT NULL DEFAULT 1,
    created_at              TIMESTAMPTZ  NOT NULL DEFAULT now(),
    CONSTRAINT bank_transaction_dedup_key_unique UNIQUE (dedup_key)
);

CREATE INDEX bank_transaction_account ON budgy.bank_transaction (bank_account_id);
CREATE INDEX bank_transaction_status ON budgy.bank_transaction (status);
