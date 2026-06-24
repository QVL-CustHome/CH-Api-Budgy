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
- Dates et horodatages en ISO 8601 (`operation_date` en `YYYY-MM-DD`, `created_at` / `updated_at` en RFC 3339 UTC).

### Filtrage et tri

Query params réutilisables : `from`, `to` (dates ISO), `account_id` (UUID).
`from` doit être antérieur ou égal à `to`, sinon `400 bad_request`.
Le filtre catégorie (`category_id`) sera ajouté en Sprint 2 quand l'endpoint le portera réellement.

### Endpoints de lecture (Sprint 1)

- `GET /v1/accounts` — liste paginée des comptes (`AccountDto`).
- `GET /v1/accounts/{account_id}/balance` — solde d'un compte (`AccountBalanceDto`).
- `GET /v1/transactions` — liste paginée des transactions (`TransactionDto`), filtrable par `account_id`, `from`, `to`.

Les catégories (S2) et budgets/agrégats (S3) réutiliseront ces mêmes primitives.
