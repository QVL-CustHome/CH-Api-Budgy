COMMENT ON COLUMN budgy.bank_account.dedup_key IS 'HMAC-SHA256 keyed digest of (consent_id, external_account_id); never store the cleartext external identifier here';

COMMENT ON COLUMN budgy.bank_transaction.dedup_key IS 'HMAC-SHA256 keyed digest of (bank_account_id, external_transaction_id); never store the cleartext external identifier here';

DELETE FROM budgy.bank_transaction;

DELETE FROM budgy.bank_account;
