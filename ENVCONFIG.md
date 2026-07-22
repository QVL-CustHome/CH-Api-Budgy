# ENVCONFIG -- CH-Api-Budgy

> Configuration de l'environnement du microservice CH-Api-Budgy (Rust / Axum, architecture hexagonale).
> Gestion des secrets et des variables d'environnement pour le faire tourner en local.
> Destinataires : developpeurs humains et agents IA.

CH-Api-Budgy est le backend du portail Budgy (gestion de budget personnel) de la flotte CustHome / QVL.
Port HTTP : `8183`.

---

## Mecanisme de configuration

Le service charge sa configuration via **Figment** (`src/config.rs`), en deux couches :

1. **`config.toml`** (versionne, non sensible) -- valeurs par defaut non sensibles (port, niveau de log).
2. **Variables d'environnement** -- chargees depuis `.env` (gitignore) par `dotenvy` au boot, puis lues par Figment.

Deux familles de variables :

- **Non sensibles** : `PORT`, `CH__SERVER__LOG_LEVEL`. Surchargent `config.toml`. Le prefixe `CH__` cible une cle de section (`CH__SECTION__CLE` -> `section.cle`).
- **Secrets** : `DATABASE_URL`, `BUDGY_ENCRYPTION_KEY`. Lus par la struct `Secrets`, **jamais** ecrits dans `config.toml`, **jamais** committes.

---

## Variables par categorie

### Variables non sensibles (surchargent config.toml)

| Variable | Obligatoire | Sensible | Defaut | Format | Description |
|----------|:-----------:|:--------:|--------|--------|-------------|
| `PORT` | non | non | `8183` | entier 1-65535 | Port d'ecoute HTTP. Surcharge `server.port` de `config.toml`. Une valeur non parsable est ignoree (le defaut de `config.toml` s'applique). |
| `CH__SERVER__LOG_LEVEL` | non | non | `INFO` | `TRACE`/`DEBUG`/`INFO`/`WARN`/`ERROR` | Niveau de log (tracing). Surcharge `server.log_level`. Une valeur invalide retombe sur `info`. |

### Secrets (struct `Secrets`, jamais dans config.toml, jamais committes)

| Variable | Obligatoire | Sensible | Defaut | Format | Description |
|----------|:-----------:|:--------:|--------|--------|-------------|
| `DATABASE_URL` | **oui** | **oui** | -- | URI PostgreSQL `postgres://user:pass@host:port/db` | Connexion a la base PostgreSQL du service (base `custhome_budgy`, port `5432`). |
| `BUDGY_ENCRYPTION_KEY` | **oui** | **oui** | -- | base64 standard decodant vers **exactement 32 octets** | Cle de chiffrement des donnees sensibles du service (credentials bancaires chiffres au repos). |

### Secret a venir (US-04, non encore consomme)

| Variable | Obligatoire | Sensible | Defaut | Format | Description |
|----------|:-----------:|:--------:|--------|--------|-------------|
| `INTERNAL_API_SECRET` | a venir | **oui** | -- | >= 32 octets UTF-8 | Secret d'authentification inter-services partage avec CH-Api-Authenticator / CH-Api-Drive (header `x-internal-secret`, comparaison temps constant). **A documenter / cabler en US-04** si Budgy doit dialoguer avec les services internes. Meme valeur que les autres services, distinct de tout secret JWT. Non lu par le code a ce jour -- aucune valeur a inventer. |

### Variable des tests d'integration (hors runtime)

| Variable | Obligatoire | Sensible | Defaut | Format | Description |
|----------|:-----------:|:--------:|--------|--------|-------------|
| `BUDGY_TEST_DATABASE_URL` | non (tests) | oui | -- | URI PostgreSQL d'administration | Base d'administration pour les tests d'integration. Le role doit avoir le privilege `CREATEDB` : le harness cree une base jetable par execution, applique les migrations `0001` -> `0014`, puis la detruit. Absente ou base indisponible -> les tests d'integration **se skippent proprement** (pas de panic). Non lue par le runtime. |

---

## Comportement fail-fast au demarrage (par secret)

Le chargement de la configuration est la **toute premiere etape** du boot (`src/main.rs`), avant l'initialisation du tracing et avant la connexion a PostgreSQL. Si la configuration est invalide, le service **refuse de demarrer** : message sur `stderr` (`eprintln!`) prefixe `Demarrage impossible -- configuration invalide`, puis **code de sortie 1**. Aucun demarrage partiel n'est possible.

| Cas | Variable | Erreur | Effet |
|-----|----------|--------|-------|
| Variable absente ou vide (apres trim) | `DATABASE_URL` | `ConfigError::MissingSecret("DATABASE_URL")` | `stderr` + `exit(1)`, refus de boot |
| Variable absente ou vide (apres trim) | `BUDGY_ENCRYPTION_KEY` | `ConfigError::MissingSecret("BUDGY_ENCRYPTION_KEY")` | `stderr` + `exit(1)`, refus de boot |
| Base64 invalide (non decodable) | `BUDGY_ENCRYPTION_KEY` | `ConfigError::InvalidEncryptionKey` | `stderr` + `exit(1)`, refus de boot |
| Mauvaise longueur (decode != 32 octets) | `BUDGY_ENCRYPTION_KEY` | `ConfigError::InvalidEncryptionKey` | `stderr` + `exit(1)`, refus de boot |
| `config.toml` introuvable / invalide | -- | `ConfigError::File` | `stderr` + `exit(1)`, refus de boot |

Erreurs post-configuration (tracing deja initialise, donc loggees via `tracing::error!` **et** `stderr`, code 1) : PostgreSQL injoignable (`DATABASE_URL` syntaxiquement correcte mais base inaccessible), echec des migrations, port deja occupe.

---

## Stockage des secrets et regle anti-commit

| Emplacement | Statut Git | Role |
|-------------|-----------|------|
| `CH-Api-Budgy/.env` | **gitignore** (`.gitignore` : `.env`, `.env.*`, sauf `!.env.example`) | Valeurs reelles consommees en local au boot (chargees par `dotenvy`). |
| `CustHome/.masterenv` (racine) | **gitignore** | Source de verite centralisee ; MasterEnv propage les valeurs vers `.env`. |
| `CH-Api-Budgy/.env.example` | **versionne** | Liste des cles avec placeholders uniquement -- **aucune valeur sensible**. |
| `CH-Api-Budgy/config.toml` | **versionne** | Valeurs non sensibles uniquement (port, log level). **Jamais** de secret. |

**Regle absolue** : `DATABASE_URL` et `BUDGY_ENCRYPTION_KEY` ne doivent **JAMAIS** etre committes. Seul `.env.example` (placeholders) est versionne. Toute valeur reelle reste dans `.env` (local) et `.masterenv` (racine), tous deux gitignore.

**Secret jamais loggue** : la struct `Secrets` a un `Debug` masque (`finish_non_exhaustive`) -- ni `DATABASE_URL` ni `BUDGY_ENCRYPTION_KEY` ne peuvent apparaitre dans un log, une trace ou un dump de debug.

---

## Generation d'une cle conforme (BUDGY_ENCRYPTION_KEY)

`BUDGY_ENCRYPTION_KEY` doit etre du base64 standard decodant vers **exactement 32 octets**.

```bash
openssl rand -base64 32
```

```bash
head -c 32 /dev/urandom | base64
```

```powershell
[Convert]::ToBase64String((1..32 | ForEach-Object { Get-Random -Maximum 256 }))
```

---

## Contenu de .env.example (versionne, placeholders uniquement)

```env
PORT=8183
CH__SERVER__LOG_LEVEL=INFO
DATABASE_URL=postgres://budgy:CHANGE_ME@localhost:5432/custhome_budgy
BUDGY_ENCRYPTION_KEY=<base64 standard de 32 octets -- ex. openssl rand -base64 32>
```

---

## Quickstart local

```bash
# 1. Copier le template
cp CH-Api-Budgy/.env.example CH-Api-Budgy/.env

# 2. Renseigner les secrets dans CH-Api-Budgy/.env :
#    - DATABASE_URL (PostgreSQL, base custhome_budgy)
#    - BUDGY_ENCRYPTION_KEY (base64 de 32 octets ; openssl rand -base64 32)

# 3. Lancer le service
cd CH-Api-Budgy && cargo run --release
```

Si un secret manque ou est mal forme, le service s'arrete immediatement (fail-fast, exit 1) avec un message sur stderr.
