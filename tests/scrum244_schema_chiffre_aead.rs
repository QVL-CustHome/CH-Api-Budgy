mod common;

use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::db::Db;
use ch_api_budgy::domain::balance::{BalanceType, NouvelleBalance};
use ch_api_budgy::domain::bank_account::NouveauBankAccount;
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::{ConsentStatus, NouveauConsent};
use ch_api_budgy::domain::ports::ecriture::ResultatInsertion;
use ch_api_budgy::domain::transaction_bancaire::{NouvelleTransactionBancaire, TransactionStatus};
use ch_api_budgy::repository::balances::SqlxBalancesRepository;
use ch_api_budgy::repository::bank_accounts::SqlxBankAccountsRepository;
use ch_api_budgy::repository::bank_transactions::SqlxBankTransactionsRepository;
use ch_api_budgy::repository::consents::SqlxConsentsRepository;
use chrono::{TimeZone, Utc};
use common::DisposableDb;
use uuid::Uuid;

const OWNER: &str = "owner-scrum-244";
const IBAN_CLAIR: &str = "FR7630006000011234567890189";
const LABEL_CLAIR: &str = "VIREMENT SALAIRE ACME SARL";
const EXTERNAL_REF_CLAIR: &str = "consent-ref-gocardless-9f2c";
const EXTERNAL_ACCOUNT_CLAIR: &str = "acct-ext-7c1d-budgy";
const EXTERNAL_TX_CLAIR: &str = "tx-ext-44ab-2026";
const MONTANT: i64 = 245_999;

macro_rules! require_db {
    () => {
        match DisposableDb::create().await {
            Some(db) => {
                db.migrate().await;
                db
            }
            None => {
                eprintln!(
                    "SCRUM-244 ignoré : variable {} absente (Postgres jetable requis)",
                    common::ENV_ADMIN_URL
                );
                return;
            }
        }
    };
}

fn crypto() -> CryptoService {
    CryptoService::from_key(&[7u8; 32]).expect("clé de test 32 octets valide")
}

fn proprietaire() -> ProprietaireId {
    ProprietaireId(OWNER.to_string())
}

async fn raw_bytea(pool: &Db, table: &str, column: &str, id: Uuid) -> Vec<u8> {
    let sql = format!("SELECT {column} FROM budgy.{table} WHERE id = $1");
    sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("colonne bytea lisible")
}

async fn raw_text(pool: &Db, table: &str, column: &str, id: Uuid) -> String {
    let sql = format!("SELECT {column} FROM budgy.{table} WHERE id = $1");
    sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("colonne texte lisible")
}

async fn key_version(pool: &Db, table: &str, id: Uuid) -> i16 {
    let sql = format!("SELECT key_version FROM budgy.{table} WHERE id = $1");
    sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("colonne key_version lisible")
}

fn ne_contient_pas_clair(blob: &[u8], clair: &str) -> bool {
    let bytes = clair.as_bytes();
    let pas_de_sous_chaine = !blob.windows(bytes.len()).any(|f| f == bytes);
    let pas_en_utf8 = !String::from_utf8_lossy(blob).contains(clair);
    pas_de_sous_chaine && pas_en_utf8
}

async fn creer_consent(db: &DisposableDb, crypto: &CryptoService) -> Uuid {
    let repo = SqlxConsentsRepository::new(db.pool.clone());
    let id = repo
        .insert(
            crypto,
            NouveauConsent {
                proprietaire: proprietaire(),
                external_ref: EXTERNAL_REF_CLAIR.to_string(),
                status: ConsentStatus::Active,
                expires_at: Some(Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap()),
            },
        )
        .await
        .expect("insertion consent");
    id.0
}

async fn creer_compte(db: &DisposableDb, crypto: &CryptoService, consent_id: Uuid) -> Uuid {
    let repo = SqlxBankAccountsRepository::new(db.pool.clone());
    let id = repo
        .insert(
            crypto,
            NouveauBankAccount {
                proprietaire: proprietaire(),
                consent: ch_api_budgy::domain::consent::ConsentId(consent_id),
                external_account_id: EXTERNAL_ACCOUNT_CLAIR.to_string(),
                iban: IBAN_CLAIR.to_string(),
                currency: "EUR".to_string(),
                next_sync_at: Some(Utc.with_ymd_and_hms(2026, 7, 1, 8, 0, 0).unwrap()),
            },
        )
        .await
        .expect("insertion bank_account");
    id.0
}

#[tokio::test]
async fn ac01_consent_modelise_et_relu_avec_ses_champs() {
    let db = require_db!();
    let crypto = crypto();
    let repo = SqlxConsentsRepository::new(db.pool.clone());

    let expires = Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap();
    let id = repo
        .insert(
            &crypto,
            NouveauConsent {
                proprietaire: proprietaire(),
                external_ref: EXTERNAL_REF_CLAIR.to_string(),
                status: ConsentStatus::Active,
                expires_at: Some(expires),
            },
        )
        .await
        .expect("insertion consent");

    let relu = repo.fetch(&crypto, &id).await.expect("lecture consent");
    let consent = relu.expect("consent présent");

    assert_eq!(consent.external_ref, EXTERNAL_REF_CLAIR);
    assert_eq!(consent.status, ConsentStatus::Active);
    assert_eq!(consent.expires_at, Some(expires));

    db.destroy().await;
}

#[tokio::test]
async fn ac01_bank_account_modelise_avec_external_id_next_sync_et_sync_count() {
    let db = require_db!();
    let crypto = crypto();
    let consent_id = creer_consent(&db, &crypto).await;
    let repo = SqlxBankAccountsRepository::new(db.pool.clone());

    let next_sync = Utc.with_ymd_and_hms(2026, 7, 1, 8, 0, 0).unwrap();
    let id = repo
        .insert(
            &crypto,
            NouveauBankAccount {
                proprietaire: proprietaire(),
                consent: ch_api_budgy::domain::consent::ConsentId(consent_id),
                external_account_id: EXTERNAL_ACCOUNT_CLAIR.to_string(),
                iban: IBAN_CLAIR.to_string(),
                currency: "EUR".to_string(),
                next_sync_at: Some(next_sync),
            },
        )
        .await
        .expect("insertion bank_account");

    let relu = repo.fetch(&crypto, &id).await.expect("lecture compte");
    let compte = relu.expect("compte présent");

    assert_eq!(compte.external_account_id, EXTERNAL_ACCOUNT_CLAIR);
    assert_eq!(compte.next_sync_at, Some(next_sync));
    assert_eq!(compte.sync_count_today, 0);

    db.destroy().await;
}

#[tokio::test]
async fn ac01_balance_modelisee_et_relue() {
    let db = require_db!();
    let crypto = crypto();
    let consent_id = creer_consent(&db, &crypto).await;
    let account_id = creer_compte(&db, &crypto, consent_id).await;
    let repo = SqlxBalancesRepository::new(db.pool.clone());

    let reference = Utc.with_ymd_and_hms(2026, 6, 20, 0, 0, 0).unwrap();
    let id = repo
        .insert(
            &crypto,
            NouvelleBalance {
                bank_account: ch_api_budgy::domain::bank_account::BankAccountId(account_id),
                balance_type: BalanceType::Available,
                amount_cents: MONTANT,
                currency: "EUR".to_string(),
                reference_date: reference,
            },
        )
        .await
        .expect("insertion balance");

    let relu = repo.fetch(&crypto, &id).await.expect("lecture balance");
    let balance = relu.expect("balance présente");

    assert_eq!(balance.amount_cents, MONTANT);
    assert_eq!(balance.balance_type, BalanceType::Available);

    db.destroy().await;
}

#[tokio::test]
async fn ac01_transaction_modelisee_avec_statut_booked_et_pending() {
    let db = require_db!();
    let crypto = crypto();
    let consent_id = creer_consent(&db, &crypto).await;
    let account_id = creer_compte(&db, &crypto, consent_id).await;
    let repo = SqlxBankTransactionsRepository::new(db.pool.clone());

    for (status, external) in [
        (TransactionStatus::Booked, "tx-booked-1"),
        (TransactionStatus::Pending, "tx-pending-1"),
    ] {
        let outcome = repo
            .insert(
                &crypto,
                NouvelleTransactionBancaire {
                    bank_account: ch_api_budgy::domain::bank_account::BankAccountId(account_id),
                    external_transaction_id: external.to_string(),
                    status,
                    label: LABEL_CLAIR.to_string(),
                    amount_cents: MONTANT,
                    currency: "EUR".to_string(),
                    booking_date: None,
                    value_date: None,
                },
            )
            .await
            .expect("insertion transaction");

        let id = match outcome {
            ResultatInsertion::Inseree(id) => id,
            ResultatInsertion::Doublon => panic!("première insertion ne doit pas être un doublon"),
        };

        let relu = repo.fetch(&crypto, &id).await.expect("lecture transaction");
        let tx = relu.expect("transaction présente");
        assert_eq!(tx.status, status);
        assert_eq!(tx.external_transaction_id, external);
    }

    db.destroy().await;
}

#[tokio::test]
async fn ac02_dedup_key_rejette_la_transaction_dupliquee() {
    let db = require_db!();
    let crypto = crypto();
    let consent_id = creer_consent(&db, &crypto).await;
    let account_id = creer_compte(&db, &crypto, consent_id).await;
    let repo = SqlxBankTransactionsRepository::new(db.pool.clone());

    let nouvelle = || NouvelleTransactionBancaire {
        bank_account: ch_api_budgy::domain::bank_account::BankAccountId(account_id),
        external_transaction_id: EXTERNAL_TX_CLAIR.to_string(),
        status: TransactionStatus::Booked,
        label: LABEL_CLAIR.to_string(),
        amount_cents: MONTANT,
        currency: "EUR".to_string(),
        booking_date: None,
        value_date: None,
    };

    let premier = repo
        .insert(&crypto, nouvelle())
        .await
        .expect("première insertion");
    assert!(matches!(premier, ResultatInsertion::Inseree(_)));

    let second = repo
        .insert(&crypto, nouvelle())
        .await
        .expect("seconde insertion");
    assert_eq!(
        second,
        ResultatInsertion::Doublon,
        "la transaction dupliquée doit être ignorée via dedup_key"
    );

    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM budgy.bank_transaction WHERE bank_account_id = $1",
    )
    .bind(account_id)
    .fetch_one(&db.pool)
    .await
    .expect("comptage transactions");
    assert_eq!(total, 1, "aucun doublon ne doit subsister en base");

    db.destroy().await;
}

#[tokio::test]
async fn ac03_colonnes_sensibles_stockees_en_bytea_chiffre() {
    let db = require_db!();
    let crypto = crypto();
    let consent_id = creer_consent(&db, &crypto).await;
    let account_id = creer_compte(&db, &crypto, consent_id).await;
    let tx_repo = SqlxBankTransactionsRepository::new(db.pool.clone());

    let outcome = tx_repo
        .insert(
            &crypto,
            NouvelleTransactionBancaire {
                bank_account: ch_api_budgy::domain::bank_account::BankAccountId(account_id),
                external_transaction_id: EXTERNAL_TX_CLAIR.to_string(),
                status: TransactionStatus::Booked,
                label: LABEL_CLAIR.to_string(),
                amount_cents: MONTANT,
                currency: "EUR".to_string(),
                booking_date: None,
                value_date: None,
            },
        )
        .await
        .expect("insertion transaction");
    let tx_id = match outcome {
        ResultatInsertion::Inseree(id) => id.0,
        ResultatInsertion::Doublon => panic!("insertion attendue"),
    };

    let external_ref = raw_bytea(&db.pool, "consent", "external_ref", consent_id).await;
    assert!(
        ne_contient_pas_clair(&external_ref, EXTERNAL_REF_CLAIR),
        "consent.external_ref doit être chiffré"
    );

    let external_account = raw_bytea(&db.pool, "bank_account", "external_account_id", account_id).await;
    assert!(
        ne_contient_pas_clair(&external_account, EXTERNAL_ACCOUNT_CLAIR),
        "bank_account.external_account_id doit être chiffré"
    );

    let iban_blob = raw_bytea(&db.pool, "bank_account", "iban_encrypted", account_id).await;
    assert!(
        ne_contient_pas_clair(&iban_blob, IBAN_CLAIR),
        "bank_account.iban_encrypted doit être chiffré"
    );

    let label_blob = raw_bytea(&db.pool, "bank_transaction", "label", tx_id).await;
    assert!(
        ne_contient_pas_clair(&label_blob, LABEL_CLAIR),
        "bank_transaction.label doit être chiffré"
    );

    let amount_blob = raw_bytea(&db.pool, "bank_transaction", "amount_cents", tx_id).await;
    assert!(
        ne_contient_pas_clair(&amount_blob, &MONTANT.to_string()),
        "bank_transaction.amount_cents doit être chiffré"
    );

    db.destroy().await;
}

#[tokio::test]
async fn ac03_cycle_chiffrement_dechiffrement_restitue_les_donnees() {
    let db = require_db!();
    let crypto = crypto();
    let consent_id = creer_consent(&db, &crypto).await;
    let account_id = creer_compte(&db, &crypto, consent_id).await;

    let account_repo = SqlxBankAccountsRepository::new(db.pool.clone());
    let compte = account_repo
        .fetch(
            &crypto,
            &ch_api_budgy::domain::bank_account::BankAccountId(account_id),
        )
        .await
        .expect("lecture compte")
        .expect("compte présent");
    assert_eq!(compte.external_account_id, EXTERNAL_ACCOUNT_CLAIR);

    let tx_repo = SqlxBankTransactionsRepository::new(db.pool.clone());
    let outcome = tx_repo
        .insert(
            &crypto,
            NouvelleTransactionBancaire {
                bank_account: ch_api_budgy::domain::bank_account::BankAccountId(account_id),
                external_transaction_id: EXTERNAL_TX_CLAIR.to_string(),
                status: TransactionStatus::Booked,
                label: LABEL_CLAIR.to_string(),
                amount_cents: MONTANT,
                currency: "EUR".to_string(),
                booking_date: None,
                value_date: None,
            },
        )
        .await
        .expect("insertion transaction");
    let tx_id = match outcome {
        ResultatInsertion::Inseree(id) => id,
        ResultatInsertion::Doublon => panic!("insertion attendue"),
    };

    let tx = tx_repo
        .fetch(&crypto, &tx_id)
        .await
        .expect("lecture transaction")
        .expect("transaction présente");
    assert_eq!(tx.label, LABEL_CLAIR);
    assert_eq!(tx.amount_cents, MONTANT);
    assert_eq!(tx.external_transaction_id, EXTERNAL_TX_CLAIR);

    db.destroy().await;
}

#[tokio::test]
async fn ac04_key_version_presente_sur_les_tables_chiffrees() {
    let db = require_db!();
    let crypto = crypto();
    let consent_id = creer_consent(&db, &crypto).await;
    let account_id = creer_compte(&db, &crypto, consent_id).await;

    let balance_repo = SqlxBalancesRepository::new(db.pool.clone());
    let balance_id = balance_repo
        .insert(
            &crypto,
            NouvelleBalance {
                bank_account: ch_api_budgy::domain::bank_account::BankAccountId(account_id),
                balance_type: BalanceType::Booked,
                amount_cents: MONTANT,
                currency: "EUR".to_string(),
                reference_date: Utc.with_ymd_and_hms(2026, 6, 20, 0, 0, 0).unwrap(),
            },
        )
        .await
        .expect("insertion balance")
        .0;

    let tx_repo = SqlxBankTransactionsRepository::new(db.pool.clone());
    let tx_id = match tx_repo
        .insert(
            &crypto,
            NouvelleTransactionBancaire {
                bank_account: ch_api_budgy::domain::bank_account::BankAccountId(account_id),
                external_transaction_id: EXTERNAL_TX_CLAIR.to_string(),
                status: TransactionStatus::Booked,
                label: LABEL_CLAIR.to_string(),
                amount_cents: MONTANT,
                currency: "EUR".to_string(),
                booking_date: None,
                value_date: None,
            },
        )
        .await
        .expect("insertion transaction")
    {
        ResultatInsertion::Inseree(id) => id.0,
        ResultatInsertion::Doublon => panic!("insertion attendue"),
    };

    assert_eq!(key_version(&db.pool, "consent", consent_id).await, 1);
    assert_eq!(key_version(&db.pool, "bank_account", account_id).await, 1);
    assert_eq!(key_version(&db.pool, "balance", balance_id).await, 1);
    assert_eq!(key_version(&db.pool, "bank_transaction", tx_id).await, 1);

    db.destroy().await;
}

#[tokio::test]
async fn ac05_iban_jamais_en_clair_seul_le_masque_est_expose() {
    let db = require_db!();
    let crypto = crypto();
    let consent_id = creer_consent(&db, &crypto).await;
    let account_id = creer_compte(&db, &crypto, consent_id).await;

    let iban_masked = raw_text(&db.pool, "bank_account", "iban_masked", account_id).await;
    assert!(
        !iban_masked.contains(IBAN_CLAIR),
        "iban_masked ne doit pas contenir l'IBAN complet en clair"
    );
    assert!(
        iban_masked.ends_with("0189"),
        "iban_masked doit conserver les derniers caractères : {iban_masked}"
    );
    assert!(
        iban_masked.contains('*'),
        "iban_masked doit être majoritairement masqué : {iban_masked}"
    );

    let iban_blob = raw_bytea(&db.pool, "bank_account", "iban_encrypted", account_id).await;
    assert!(
        ne_contient_pas_clair(&iban_blob, IBAN_CLAIR),
        "iban_encrypted ne doit jamais exposer l'IBAN en clair"
    );

    let account_repo = SqlxBankAccountsRepository::new(db.pool.clone());
    let compte = account_repo
        .fetch(
            &crypto,
            &ch_api_budgy::domain::bank_account::BankAccountId(account_id),
        )
        .await
        .expect("lecture compte")
        .expect("compte présent");
    assert_eq!(compte.iban_masked, iban_masked);
    assert!(
        !compte.external_account_id.contains(IBAN_CLAIR),
        "l'entité exposée ne doit pas porter l'IBAN en clair"
    );

    db.destroy().await;
}
