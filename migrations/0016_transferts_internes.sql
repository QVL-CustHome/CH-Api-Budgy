-- Transferts internes : un virement entre deux comptes du meme proprietaire
-- n'est ni une depense ni un revenu (l'argent change juste de poche). Sans ce
-- marquage, il est compte deux fois : en depense cote emetteur et en revenu
-- cote destinataire, ce qui gonfle le reste a depenser et le previsionnel.
ALTER TABLE budgy.bank_transaction
    ADD COLUMN IF NOT EXISTS is_internal_transfer BOOLEAN NOT NULL DEFAULT false;

CREATE INDEX IF NOT EXISTS bank_transaction_internal_transfer
    ON budgy.bank_transaction (is_internal_transfer);

-- Categorie systeme a UUID fige : echappatoire manuelle pour les virements que
-- l'appariement automatique ne peut pas detecter (compte destinataire non
-- rattache a Budgy). Categoriser une transaction ainsi l'exclut des calculs.
INSERT INTO budgy.category (id, owner_id, name, kind, color, icon)
VALUES (
    '00000000-0000-4000-8000-000000000001',
    NULL,
    'Virements internes',
    'depense',
    '#546E7A',
    'exchange'
)
ON CONFLICT (id) DO NOTHING;
