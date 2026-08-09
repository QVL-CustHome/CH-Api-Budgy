-- Enveloppes budgétaires (« budgets » dans l'interface).
--
-- À ne pas confondre avec budgy.budgets, qui plafonne une catégorie sur un mois
-- donné. Une enveloppe n'est rattachée à aucun mois : elle vit jusqu'à sa
-- suppression, et l'utilisateur y range lui-même les transactions — aucune
-- règle d'automatisation ne s'y applique.
CREATE TABLE budgy.enveloppe (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id      TEXT        NOT NULL,
    nom           TEXT        NOT NULL,
    icon          TEXT        NOT NULL,
    color         TEXT        NOT NULL,
    montant_cents BIGINT      NOT NULL CHECK (montant_cents >= 0),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT enveloppe_owner_nom_unique UNIQUE (owner_id, nom)
);

CREATE INDEX enveloppe_owner ON budgy.enveloppe (owner_id);

-- Une transaction peut porter une enveloppe **en plus** de sa catégorie : les
-- deux classements sont indépendants. Supprimer l'enveloppe libère la
-- transaction sans la perdre.
ALTER TABLE budgy.bank_transaction
    ADD COLUMN enveloppe_id UUID REFERENCES budgy.enveloppe (id) ON DELETE SET NULL;

CREATE INDEX bank_transaction_enveloppe ON budgy.bank_transaction (enveloppe_id);
