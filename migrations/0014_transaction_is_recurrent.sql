ALTER TABLE budgy.bank_transaction
    ADD COLUMN is_recurrent        BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN recurrence_interval TEXT;

ALTER TABLE budgy.bank_transaction
    ADD CONSTRAINT bank_transaction_recurrence_interval_valide
    CHECK (recurrence_interval IS NULL OR recurrence_interval IN ('monthly'));

ALTER TABLE budgy.bank_transaction
    ADD CONSTRAINT bank_transaction_recurrence_coherente
    CHECK (
        (is_recurrent = false AND recurrence_interval IS NULL)
        OR (is_recurrent = true AND recurrence_interval IS NOT NULL)
    );

CREATE INDEX bank_transaction_is_recurrent ON budgy.bank_transaction (is_recurrent);
