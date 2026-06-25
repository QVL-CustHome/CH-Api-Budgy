ALTER TABLE budgy.bank_account ADD COLUMN dedup_key TEXT;

UPDATE budgy.bank_account
SET dedup_key = consent_id::text || ':' || id::text
WHERE dedup_key IS NULL;

ALTER TABLE budgy.bank_account ALTER COLUMN dedup_key SET NOT NULL;

ALTER TABLE budgy.bank_account
    ADD CONSTRAINT bank_account_consent_dedup_unique UNIQUE (consent_id, dedup_key);
