ALTER TABLE budgy.bank_account
    ADD COLUMN last_sync_day DATE,
    ADD COLUMN last_sync_at  TIMESTAMPTZ;

CREATE INDEX bank_account_next_sync_at ON budgy.bank_account (next_sync_at);
