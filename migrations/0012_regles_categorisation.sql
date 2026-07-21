CREATE TABLE budgy.regles_categorisation (
    id            UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id      TEXT         NOT NULL,
    label_pattern TEXT         NOT NULL,
    category_id   UUID         NOT NULL REFERENCES budgy.category (id) ON DELETE CASCADE,
    priority      INT          NOT NULL DEFAULT 0,
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX regles_categorisation_owner ON budgy.regles_categorisation (owner_id);
