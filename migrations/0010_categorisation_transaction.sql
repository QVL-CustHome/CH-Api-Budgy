ALTER TABLE budgy.bank_transaction
    ADD COLUMN category_id           UUID REFERENCES budgy.category (id) ON DELETE SET NULL,
    ADD COLUMN categorization_source TEXT NOT NULL DEFAULT 'none',
    ADD COLUMN rule_id               UUID;

CREATE INDEX bank_transaction_category ON budgy.bank_transaction (category_id);

CREATE FUNCTION budgy.reinitialiser_categorisation_transaction()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    UPDATE budgy.bank_transaction
    SET categorization_source = 'none', rule_id = NULL
    WHERE category_id = OLD.id;
    RETURN OLD;
END;
$$;

CREATE TRIGGER category_suppression_reinitialise_categorisation
    BEFORE DELETE ON budgy.category
    FOR EACH ROW
    EXECUTE FUNCTION budgy.reinitialiser_categorisation_transaction();
