-- Empreinte d'un compte bancaire, indépendante du propriétaire et du consentement.
--
-- La clé de dédoublonnage existante (`dedup_key`) dérive du `consent_id` : deux
-- personnes rattachant le MÊME compte bancaire produisent donc deux clés
-- différentes, et rien ne permet de s'en apercevoir. C'est ce qui a laissé le
-- compte d'un utilisateur être importé chez un autre sans le moindre signal
-- (incident du 2026-08-11).
--
-- Cette empreinte-ci ne dépend que de l'identifiant du compte chez la banque.
-- Elle sert à répondre à une seule question au moment du rattachement :
-- « ce compte appartient-il déjà à quelqu'un d'autre ? »
--
-- Volontairement PAS de contrainte d'unicité : un même titulaire re-rattache
-- légitimement son compte à chaque renouvellement de consentement (tous les
-- 90 jours), ce qui crée une nouvelle ligne avec la même empreinte. Le contrôle
-- est applicatif, et ne rejette que le cas « autre propriétaire ».
ALTER TABLE budgy.bank_account
    ADD COLUMN IF NOT EXISTS empreinte_compte text;

CREATE INDEX IF NOT EXISTS bank_account_empreinte_idx
    ON budgy.bank_account (empreinte_compte)
    WHERE empreinte_compte IS NOT NULL;
