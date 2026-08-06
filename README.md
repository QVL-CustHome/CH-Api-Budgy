# CH-Api-Budgy

Microservice backend du portail Budgy (gestion de budget personnel) de la flotte CustHome / QVL.

Rust / Axum, architecture hexagonale (domaine / ports / adapters). En production sur `ch-budgy.qvl-project.com`, port `8183`.

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
- `GET /v1/balance` — solde consolidé de tous les comptes du propriétaire : `{ total_cents, total_a_venir_cents?, accounts: [ { id, iban_masked, currency, balance, solde_a_venir_cents? } ] }`. Un compte sans solde connu compte pour `0`. Le **solde à venir** est le solde bancaire de type `expected` (opérations en attente incluses) : il n'est présent que pour les banques qui l'exposent (Boursorama oui, Crédit Agricole non), d'où des champs omis quand l'information manque. `total_a_venir_cents` n'apparaît que si au moins un compte l'expose, et retient pour les autres leur solde courant.
- `GET /v1/budgets?mois=YYYY-MM` — budgets mensuels par catégorie : `{ data: [ { id, category_id, montant_cents, mois, created_at, updated_at } ], total }`.
- `POST /v1/budgets` — crée ou met à jour le budget d'une catégorie pour un mois (`{ category_id, montant_cents, mois }`). `201` avec le budget ; `404 not_found` si la catégorie n'appartient pas au propriétaire.
- `GET /v1/budgets/remaining?month=YYYY-MM` — reste à dépenser par catégorie : `{ month, total: { montant_prevu_cents, depense_cents, reste_cents }, categories: [ { category_id, category_name, kind, color, icon, montant_prevu_cents, depense_cents, reste_cents, depassement_cents, depasse } ] }`. Deux modes : si des budgets sont définis pour le mois, le montant prévu est le budget ; sinon il est **prédit** (voir *Prédiction par médiane*). Le `total` est **exactement la somme des lignes renvoyées** — y agréger les dépenses non catégorisées donnerait un total que le détail ne permet pas de recouper.
- `GET /v1/expenses/by-category?month=YYYY-MM` — dépenses du mois réparties par catégorie (graphique home made côté front) : `{ month, total_cents, categories: [ { category_id, category_name, kind, color, icon, amount_cents } ] }`. Les transactions sans catégorie sont regroupées sous une ligne à champs `null`.
- `GET /v1/forecast?month=YYYY-MM` — budget prévisionnel mensuel. `solde_previsionnel_cents = revenus_recurrents_cents − depenses_recurrentes_cents − budgets_cents` : `{ month, solde_previsionnel_cents, revenus_recurrents_cents, depenses_recurrentes_cents, budgets_cents, donnees_suffisantes, categories: [ … ] }`. Les deux côtés viennent de sources **différentes** :
  - **dépenses récurrentes** — détection d'occurrences à montant fixe (le montant retenu est la dernière occurrence par tiers) ;
  - **revenus récurrents** — médiane des crédits mensuels, catégorie par catégorie, sur les catégories de `kind = revenu` uniquement. La détection à montant fixe est structurellement aveugle à un salaire, qui varie de plusieurs dizaines d'euros et porte le mois dans son libellé. Un crédit rangé dans une catégorie de dépense est un remboursement, pas une rentrée : il ne compte pas.

  Les crédits sont donc volontairement ignorés côté récurrences, sous peine d'être comptés deux fois. `donnees_suffisantes` est `false` quand il n'y a ni récurrence détectée ni revenu prédit.
- `POST /v1/transactions/recategoriser` — réconciliation idempotente, appelée par le portail à son chargement. Trois temps : (1) appariement des **virements internes**, (2) ré-application de **toutes** les règles aux transactions non catégorisées, (3) mise en « Salaire » des crédits restants. Ne touche que les transactions en `categorization_source = 'none'` : un choix manuel ou une règle déjà appliquée ne sont jamais réécrits. Réponse `{ categorisees }`.
- `POST /v1/accounts/{account_id}/transactions/{transaction_id}/rule` — crée une règle **à partir d'une transaction** : le motif est dérivé automatiquement de son libellé (`extraire_tiers`), l'utilisateur n'a rien à saisir. La règle est ensuite appliquée rétroactivement.

## Moteur de catégorisation par règles (SCRUM-231/232/233)

Une règle appartient à un propriétaire et associe un `label_pattern` à une `category_id` avec une `priority`. Comme les libellés de transaction sont chiffrés en base, le matching se fait **en applicatif après déchiffrement** ; aucun matching SQL n'est possible.

Le matcher (`RegleCategorisation::correspond`) compare une **sous-chaîne insensible à la casse des tiers extraits de part et d'autre** (`extraire_tiers`), et non des libellés bruts. C'est indispensable : un motif dérivé d'un libellé nettoyé (« CARTE INTERMARCHE ») n'est jamais une sous-chaîne du libellé brut (« CARTE 07/07/26 INTERMARCHE CB*7513 »), où une date s'intercale — et le format varie d'une banque à l'autre pour le même marchand. Conséquence attendue : un motif réduit à un **préfixe d'opération** (« ACHAT », « CARTE ») ne matche plus rien, puisque ces préfixes sont retirés des deux côtés.

Le classement des règles candidates est porté par le domaine (`selectionner_regle`), totalement déterministe : `priority` DESC, puis `created_at` DESC, puis `id`. Il ne dépend pas de l'ordre de retour SQL.

Deux volets d'application :

- **Nouvelles transactions** : à chaque insertion effective, la règle du propriétaire la mieux classée est appliquée. Une catégorisation manuelle n'est jamais réécrite (`categorization_source <> 'manual'`). L'échec de cette étape est non-bloquant (loggé en `warn`, l'insertion reste acquise).
- **Rétroactif** : à la création d'une règle, les transactions non catégorisées du propriétaire (`categorization_source = 'none'`) sont recatégorisées par lot. Non-bloquant : la création répond `201` même si le batch échoue. Plafond de `5000` transactions par lot (au-delà, un `warn` est émis).

## Extraction du tiers (`domain::libelle`)

`extraire_tiers` réduit un libellé bancaire au marchand / à la contrepartie : retrait des préfixes d'opération (`VIREMENT EN VOTRE FAVEUR`, `PAIEMENT PAR CARTE`…), des civilités, puis filtrage des tokens volatiles (tout token contenant un chiffre, masques de carte `XXXX`/`****`, noms de mois, mots de bruit `REF`/`MANDAT`/`RUM`…), limité à 5 tokens. Ne renvoie jamais de chaîne vide : à défaut, repli sur les premiers mots normalisés.

C'est la **clé de stabilité** de trois mécanismes, tous cassés sans elle parce que les libellés varient chaque mois : le matching des règles, le regroupement des récurrences (`normaliser_marchand` y délègue) et l'affichage (`clean_label` exposé sur chaque transaction).

## Virements internes (`domain::transfert_interne`)

Un virement entre deux comptes du même propriétaire apparaît deux fois : en débit côté émetteur, en crédit côté destinataire. Sans traitement il est compté **à la fois comme une dépense et comme un revenu**, ce qui gonfle le reste à dépenser et fausse le prévisionnel.

L'appariement retient un débit et un crédit de **même montant**, sur des **comptes différents**, à **±4 jours** (les deux banques ne datent pas l'opération le même jour). Chaque mouvement n'est apparié qu'une fois. Les deux faces sont marquées `is_internal_transfer` (migration `0016`) et **exclues** des dépenses, des revenus, de la détection de récurrences et de la catégorisation automatique. Le marquage retire aussi la catégorisation *automatique* héritée (un virement pris pour un salaire) ; un choix manuel est conservé.

Échappatoire manuelle pour un virement que l'appariement ne peut pas détecter (compte destinataire non rattaché à Budgy) : la catégorie système **« Virements internes »** (UUID figé `00000000-0000-4000-8000-000000000001`). Y ranger une transaction l'exclut des calculs ; l'en sortir la réintègre.

## Prédiction par médiane (`domain::agregation`)

Le reste à dépenser (sans budget défini) et les revenus du prévisionnel se prédisent sur la **médiane des `MOIS_HISTORIQUE = 3` derniers mois**, catégorie par catégorie. Un mois sans montant pour une catégorie compte pour **zéro**, et seules les médianes strictement positives sont retenues.

La médiane, et non la moyenne ni le seul mois précédent : une dépense exceptionnelle isolée (médiane de `[372, 0, 0]` = `0`) ne doit pas devenir une enveloppe mensuelle, alors qu'une charge régulière l'est bien (médiane de `[200, 210, 205]` = `205`).

## Tests d'intégration

Les tests d'intégration nécessitent un PostgreSQL accessible via la variable `BUDGY_TEST_DATABASE_URL`, avec un rôle disposant du privilège `CREATEDB` : le harness (`tests/common/mod.rs`) crée une base jetable par exécution, y applique les migrations `0001` → `0016`, puis la détruit.

Sans cette variable, ou si la base est indisponible / le privilège `CREATEDB` manquant, les tests d'intégration **se skippent proprement** (message sur `stderr`, aucun panic).

## Décisions / Sécurité

- **2026-08-06** : le `total` de `/v1/budgets/remaining` devient la **somme exacte des lignes renvoyées**. Il agrégeait auparavant toutes les dépenses du mois, non catégorisées comprises — invisibles dans le détail : le total dépassait de ~1 300 €/mois la somme des lignes et surestimait le reste à dépenser au point de le rendre trompeur. Contrepartie assumée : l'enveloppe ne couvre que ce qu'on sait rattacher, et se complète à mesure que l'utilisateur catégorise.
- **2026-08-06** : revenus du prévisionnel calculés par **médiane des crédits par catégorie de revenu**, plus par détection de récurrence. Un salaire réel (3 occurrences, 3 libellés distincts, 92,57 € d'écart entre montants) ne pouvait pas être détecté : la tolérance de montant est de 1 € et le regroupement exige un tiers identique. Le prévisionnel affichait `revenus = 0` et un solde négatif, indéfiniment — attendre des mois supplémentaires n'y aurait rien changé.
- **2026-08-06** : reste à dépenser prédit sur la **médiane de 3 mois** au lieu du seul mois précédent, pour qu'une dépense exceptionnelle ne devienne pas une enveloppe mensuelle.
- **2026-08-05** : **virements internes** appariés et exclus des calculs (migration `0016`), avec catégorie système « Virements internes » comme échappatoire manuelle. Corrige un double comptage réel : un virement compté en dépense d'un côté et en « Salaire » de l'autre.
- **2026-08-05** : matching des règles porté sur les **tiers** (`extraire_tiers`) des deux côtés, au lieu des libellés bruts. Les règles dérivées d'un libellé nettoyé ne matchaient jamais les paiements carte (date intercalée), toutes banques confondues : la catégorisation automatique était en pratique inopérante sur ce format. Effet de bord accepté : un motif réduit à un préfixe d'opération ne matche plus.
- **2026-08-05** : regroupement des récurrences porté sur `extraire_tiers` (via `normaliser_marchand`), pour reconnaître un marchand dont le libellé porte le mois ou une référence variable.
- **2026-08-05** : `POST /v1/transactions/recategoriser` devient une réconciliation complète (virements internes → règles → crédits), appelée au chargement du portail : les données se réparent d'elles-mêmes sans intervention.
- **2026-08-05** : **solde à venir** (`expected`) exposé par compte et consolidé. Toutes les banques ne le fournissent pas — les champs sont omis plutôt que remplis d'une valeur par défaut trompeuse. Le détail des prélèvements à venir n'est **pas** accessible en PSD2 : seul leur effet net sur le solde l'est.

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
