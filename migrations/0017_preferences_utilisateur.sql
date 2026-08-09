-- Réglages propres à chaque utilisateur. Aujourd'hui le seul est le jour de
-- départ du mois budgétaire : un salaire versé le 28 rend le découpage
-- calendaire inutilisable pour suivre un budget.
CREATE TABLE budgy.preferences_utilisateur (
    owner_id        TEXT        PRIMARY KEY,
    jour_debut_mois SMALLINT    NOT NULL DEFAULT 1
                                CHECK (jour_debut_mois BETWEEN 1 AND 31),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
