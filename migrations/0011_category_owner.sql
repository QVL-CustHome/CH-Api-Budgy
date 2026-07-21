ALTER TABLE budgy.category ADD COLUMN owner_id TEXT;

CREATE INDEX category_owner ON budgy.category (owner_id);
