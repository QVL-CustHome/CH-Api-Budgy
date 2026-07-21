CREATE TABLE budgy.category (
    id         UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT         NOT NULL,
    kind       TEXT         NOT NULL,
    color      TEXT         NOT NULL,
    icon       TEXT         NOT NULL,
    created_at TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX category_kind ON budgy.category (kind);

INSERT INTO budgy.category (name, kind, color, icon) VALUES
    ('Salaire',          'revenu',  '#2E7D32', 'briefcase'),
    ('Autres revenus',   'revenu',  '#66BB6A', 'plus-circle'),
    ('Loyer',            'depense', '#C62828', 'home'),
    ('Courses',          'depense', '#EF6C00', 'shopping-cart'),
    ('Transport',        'depense', '#1565C0', 'car'),
    ('Loisirs',          'depense', '#6A1B9A', 'gamepad-2'),
    ('Santé',            'depense', '#00838F', 'heart-pulse'),
    ('Restaurants',      'depense', '#AD1457', 'utensils'),
    ('Factures',         'depense', '#455A64', 'file-text'),
    ('Autres dépenses',  'depense', '#757575', 'ellipsis');
