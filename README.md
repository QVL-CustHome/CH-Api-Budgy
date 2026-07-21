# CH-Api-Budgy

Microservice backend du portail Budgy (gestion de budget personnel) de la flotte CustHome / QVL.

Statut : initialisation du socle technique (Sprint 0 - Socle technique).

Le scaffolding applicatif (Rust / Axum, architecture hexagonale) est réalisé sur la branche de feature `feat/scrum-196-socle-budgy`.

## Conventions d'API de lecture

Primitives partagées par tous les endpoints de lecture (module `src/api`).

### Pagination

Query params `limit` et `offset`.

- `limit` par défaut : 50, maximum : 200.
- `offset` par défaut : 0.
- `limit = 0` ou `limit > 200` renvoie `400 bad_request`.

### Enveloppe de liste

```json
{ "data": [ ... ], "total": 1234 }
```

`total` est le nombre total d'éléments correspondant au filtre, indépendant de la pagination.

### Format d'erreur

```json
{ "code": "bad_request", "message": "limit ne peut pas dépasser 200" }
```

| code | statut HTTP |
| --- | --- |
| `bad_request` | 400 |
| `unauthorized` | 401 |
| `forbidden` | 403 |
| `not_found` | 404 |
| `conflict` | 409 |
| `internal_error` | 500 |

### Montants et dates

- Montants en entier de centimes (`*_cents`), jamais en flottant.
- Dates et horodatages en ISO 8601 (`booking_date` / `value_date` en `YYYY-MM-DD`, `at` en RFC 3339 UTC).

### Pagination

Query params réutilisables : `limit` (défaut 50, max 200), `offset` (défaut 0).
`limit=0` ou `limit > 200` renvoie `400 bad_request`.

### Endpoints de lecture (Sprint 1)

Comptes bancaires chiffrés (IBAN, libellés et montants déchiffrés côté back avant exposition ; IBAN jamais exposé en clair). Périmètre filtré par le `sub` du JWT (anti-IDOR).

- `GET /v1/accounts` — liste paginée des comptes du `sub` avec leur solde courant : `{ data: [ { id, iban_masked, currency, balance: { amount_cents, type, at } } ], total }`.
- `GET /v1/accounts/{account_id}` — détail d'un compte (même forme qu'un élément de la liste) ; `404 not_found` si le compte n'appartient pas au `sub`.
- `GET /v1/accounts/{account_id}/transactions` — transactions paginées du compte, triées par date décroissante : `{ data: [ { id, label, amount_cents, currency, status, booking_date, value_date } ], total }` ; `404 not_found` si le compte n'appartient pas au `sub`.

Les catégories (S2) et budgets/agrégats (S3) réutiliseront ces mêmes primitives.

## Décisions / Sécurité

- **2026-07-21 (SCRUM-232)** : `budgy.regles_categorisation.label_pattern` est stocké en clair alors que `bank_transaction.label` est chiffré (BYTEA). Risque PII résiduel (un nom de marchand apparaît en clair au niveau accès-DB-au-repos) accepté sciemment pour la simplicité (matching applicatif). À réévaluer lors de l'audit sécurité de fin de sprint (option : chiffrer le pattern via CryptoService, déchiffrement en mémoire au matching).
