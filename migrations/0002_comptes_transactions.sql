CREATE TABLE budgy.account (
    id            UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id      TEXT         NOT NULL,
    label         TEXT         NOT NULL,
    institution   TEXT         NOT NULL,
    iban          TEXT,
    currency      TEXT         NOT NULL,
    balance_cents BIGINT       NOT NULL DEFAULT 0,
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX account_owner ON budgy.account (owner_id);

CREATE TABLE budgy.transaction (
    id                 UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id         UUID         NOT NULL REFERENCES budgy.account (id) ON DELETE CASCADE,
    owner_id           TEXT         NOT NULL,
    label              TEXT         NOT NULL,
    amount_cents       BIGINT       NOT NULL,
    direction          TEXT         NOT NULL,
    currency           TEXT         NOT NULL,
    operation_date     DATE         NOT NULL,
    value_date         DATE,
    category_id        UUID,
    external_reference TEXT,
    created_at         TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX transaction_owner ON budgy.transaction (owner_id);
CREATE INDEX transaction_account ON budgy.transaction (account_id);
CREATE INDEX transaction_operation_date ON budgy.transaction (operation_date);
