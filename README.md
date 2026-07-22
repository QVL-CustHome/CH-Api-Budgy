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

Les catégories (S2) et budgets/agrégats (S3) réutilisent ces mêmes primitives.

### Endpoints d'agrégats et budgets (Sprint 3)

Tous filtrés par le `sub` du JWT (anti-IDOR). Les paramètres de mois attendent `YYYY-MM`.

- `GET /v1/transactions` — transactions paginées du propriétaire, tous comptes confondus, avec filtres et tri : `account_id`, `category_id`, `from`/`to` (`YYYY-MM-DD`, `from` ≤ `to`), `type` (`credit`/`debit`), `sort` (`date`|`amount`, défaut `date`), `order` (`asc`|`desc`, défaut `desc`), `limit`/`offset`. Réponse `{ data: [ { id, label, amount_cents, currency, status, booking_date, value_date, category_id, categorization_source } ], total }`.
- `GET /v1/balance` — solde consolidé de tous les comptes du propriétaire : `{ total_cents, accounts: [ { id, iban_masked, currency, balance } ] }`. Un compte sans solde connu compte pour `0`.
- `GET /v1/budgets?mois=YYYY-MM` — budgets mensuels par catégorie : `{ data: [ { id, category_id, montant_cents, mois, created_at, updated_at } ], total }`.
- `POST /v1/budgets` — crée ou met à jour le budget d'une catégorie pour un mois (`{ category_id, montant_cents, mois }`). `201` avec le budget ; `404 not_found` si la catégorie n'appartient pas au propriétaire.
- `GET /v1/budgets/remaining?month=YYYY-MM` — reste à dépenser par catégorie budgétée : `{ month, categories: [ { category_id, category_name, kind, color, icon, montant_prevu_cents, depense_cents, reste_cents, depassement_cents, depasse } ] }`.
- `GET /v1/expenses/by-category?month=YYYY-MM` — dépenses du mois réparties par catégorie (graphique home made côté front) : `{ month, total_cents, categories: [ { category_id, category_name, kind, color, icon, amount_cents } ] }`. Les transactions sans catégorie sont regroupées sous une ligne à champs `null`.
- `GET /v1/forecast?month=YYYY-MM` — budget prévisionnel mensuel. `solde_previsionnel_cents = revenus_recurrents_cents − depenses_recurrentes_cents − budgets_cents`, avec le détail par catégorie et un flag `donnees_suffisantes` (`false` si aucune récurrence détectée) : `{ month, solde_previsionnel_cents, revenus_recurrents_cents, depenses_recurrentes_cents, budgets_cents, donnees_suffisantes, categories: [ { category_id, category, revenus_recurrents_cents, depenses_recurrentes_cents, budget_cents } ] }`. Le montant récurrent d'un marchand est sa dernière occurrence (marchand normalisé) ; le classement revenu/dépense suit le `kind` de la catégorie, avec repli sur le signe du montant pour les transactions non catégorisées.

## Moteur de catégorisation par règles (SCRUM-231/232/233)

Une règle appartient à un propriétaire et associe un `label_pattern` à une `category_id` avec une `priority`. Le matcher est une **sous-chaîne insensible à la casse** (`RegleCategorisation::correspond`). Comme les libellés de transaction sont chiffrés en base, le matching se fait **en applicatif après déchiffrement** ; aucun matching SQL n'est possible.

Le classement des règles candidates est porté par le domaine (`selectionner_regle`), totalement déterministe : `priority` DESC, puis `created_at` DESC, puis `id`. Il ne dépend pas de l'ordre de retour SQL.

Deux volets d'application :

- **Nouvelles transactions** : à chaque insertion effective, la règle du propriétaire la mieux classée est appliquée. Une catégorisation manuelle n'est jamais réécrite (`categorization_source <> 'manual'`). L'échec de cette étape est non-bloquant (loggé en `warn`, l'insertion reste acquise).
- **Rétroactif** : à la création d'une règle, les transactions non catégorisées du propriétaire (`categorization_source = 'none'`) sont recatégorisées par lot. Non-bloquant : la création répond `201` même si le batch échoue. Plafond de `5000` transactions par lot (au-delà, un `warn` est émis).

## Tests d'intégration

Les tests d'intégration nécessitent un PostgreSQL accessible via la variable `BUDGY_TEST_DATABASE_URL`, avec un rôle disposant du privilège `CREATEDB` : le harness (`tests/common/mod.rs`) crée une base jetable par exécution, y applique les migrations `0001` → `0014`, puis la détruit.

Sans cette variable, ou si la base est indisponible / le privilège `CREATEDB` manquant, les tests d'intégration **se skippent proprement** (message sur `stderr`, aucun panic).

## Décisions / Sécurité

- **2026-07 (SCRUM-234)** : budgets mensuels par catégorie (`GET`/`POST /v1/budgets`), un montant prévu par couple catégorie/mois, upsert idempotent.
- **2026-07 (SCRUM-235)** : reste à dépenser par catégorie budgétée (`GET /v1/budgets/remaining`), calcul domaine `montant_prevu − dépenses du mois` avec dépassement explicite.
- **2026-07 (SCRUM-236)** : détection des récurrences ajoutée, marquage porté par la migration `0014_transaction_is_recurrent.sql`.
- **2026-07 (SCRUM-238)** : solde consolidé tous comptes (`GET /v1/balance`), compte sans solde connu compté pour `0`.
- **2026-07 (SCRUM-239)** : dépenses mensuelles par catégorie (`GET /v1/expenses/by-category`), agrégat servant le graphique home made du front (pas de dépendance de charting).
- **2026-07 (SCRUM-240)** : liste transverse des transactions (`GET /v1/transactions`) avec filtres (`account_id`, `category_id`, `from`/`to`, `type`) et tri (`date`/`amount`, `asc`/`desc`), réutilisant les primitives de pagination.
- **2026-07 (SCRUM-237)** : budget prévisionnel (`GET /v1/forecast`), `solde = revenus récurrents − dépenses récurrentes − budgets`. Récurrent = dernière occurrence par marchand normalisé ; classement revenu/dépense par `kind` de catégorie (repli sur le signe du montant si non catégorisée) ; flag `donnees_suffisantes` à `false` sans récurrence détectée.
- **2026-07 (SEC-BUDGY-S3-01, SCRUM-350, dette ouverte)** : la vérification de signature de l'event `auth/user/deleted` reste à confirmer côté Budgy. Dette d'audit S3 ouverte, à traiter.
- **2026-07 (SCRUM-351)** : convention de ports de l'écosystème à formaliser (Budgy sur `8183`) ; décision de convention tracée, alignement à finaliser avec le lead.
- **2026-07-21 (SCRUM-233)** : moteur d'application des règles livré (nouvelles transactions + rétroactif à la création de règle). Le matching est **applicatif après déchiffrement** des libellés : les `label` sont chiffrés en base, un matching SQL est donc impossible.
- **2026-07-21 (clôture S2)** : le classement des règles est porté par le domaine (`selectionner_regle`), indépendant de l'ordre de retour SQL. Les valeurs de `categorization_source` (`manual` / `rule` / `none`) ont une source unique : l'enum `CategorizationSource`.
- **2026-07-21 (SEC-001, dette assumée)** : `budgy.regles_categorisation.label_pattern` reste stocké **en clair** alors que `bank_transaction.label` est chiffré (BYTEA). Décision cohérente avec SCRUM-232. L'audit sécu de clôture l'a classé **Medium** (PII potentielle, incohérent avec le chiffrement des labels de transaction). Chiffrement **reporté** à un sprint ultérieur ; hacher est exclu car casserait le matching par sous-chaîne. À réévaluer.
- **2026-07-21 (dette archi, à trancher)** : pattern trait-port + adapter concret généralisé mais non consommé en `dyn` (observation d'audit, hors périmètre US). Décision de convention à trancher avec le lead.
