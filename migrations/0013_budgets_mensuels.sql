CREATE TABLE budgy.budgets (
    id                  UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id            TEXT        NOT NULL,
    category_id         UUID        NOT NULL REFERENCES budgy.category (id) ON DELETE CASCADE,
    montant_prevu_cents BIGINT      NOT NULL CHECK (montant_prevu_cents >= 0),
    mois                DATE        NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT budgets_owner_category_mois_unique UNIQUE (owner_id, category_id, mois)
);

CREATE INDEX budgets_owner ON budgy.budgets (owner_id);
CREATE INDEX budgets_owner_mois ON budgy.budgets (owner_id, mois);
