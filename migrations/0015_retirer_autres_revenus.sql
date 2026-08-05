-- Retire la catégorie par défaut « Autres revenus » (seedée en 0009).
-- Les transactions éventuellement rattachées repassent à category_id NULL
-- (ON DELETE SET NULL) et le trigger réinitialise leur catégorisation.
DELETE FROM budgy.category
WHERE name = 'Autres revenus'
  AND kind = 'revenu'
  AND coalesce(owner_id, '') = '';
