mod common;

use std::sync::Arc;

use ch_api_budgy::adapters::bank::mock::MockBankDataSource;
use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::domain::balance::{BalanceType, NouvelleBalance};
use ch_api_budgy::domain::bank_account::{BankAccountId, NouveauBankAccount};
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::{ConsentId, ConsentStatus, NouveauConsent};
use ch_api_budgy::domain::effacement::EffacementProprietaire;
use ch_api_budgy::domain::ports::bank_data_source::BankDataSource;
use ch_api_budgy::domain::ports::ecriture::{
    BalancesWriteRepository, BankAccountsWriteRepository, BankTransactionsWriteRepository,
    ConsentsWriteRepository, ResultatInsertion,
};
use ch_api_budgy::domain::transaction_bancaire::{NouvelleTransactionBancaire, TransactionStatus};
use ch_api_budgy::repository::balances::SqlxBalancesWriteAdapter;
use ch_api_budgy::repository::bank_accounts::SqlxBankAccountsWriteAdapter;
use ch_api_budgy::repository::bank_transactions::SqlxBankTransactionsWriteAdapter;
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
use chrono::{TimeZone, Utc};
use common::DisposableDb;

const OWNER_A: &str = "owner-scrum-253-a";
const OWNER_B: &str = "owner-scrum-253-b";

macro_rules! require_db {
    () => {
        match DisposableDb::create().await {
            Some(db) => {
                db.migrate().await;
                db
            }
            None => {
                eprintln!(
                    "SCRUM-253 cascade ignoré : variable {} absente (Postgres jetable requis)",
                    common::ENV_ADMIN_URL
                );
                return;
            }
        }
    };
}

fn crypto() -> Arc<CryptoService> {
    Arc::new(CryptoService::from_key(&[7u8; 32]).expect("clé de test 32 octets valide"))
}

async fn semer_proprietaire(db: &DisposableDb, crypto: &Arc<CryptoService>, owner: &str) {
    let proprietaire = ProprietaireId(owner.to_string());
    let consents = SqlxConsentsWriteAdapter::new(db.pool.clone(), crypto.clone());
    let comptes = SqlxBankAccountsWriteAdapter::new(db.pool.clone(), crypto.clone());
    let balances = SqlxBalancesWriteAdapter::new(db.pool.clone(), crypto.clone());
    let transactions = SqlxBankTransactionsWriteAdapter::new(db.pool.clone(), crypto.clone());

    let consent_id = ConsentsWriteRepository::enregistrer(
        &consents,
        NouveauConsent {
            proprietaire: proprietaire.clone(),
            external_ref: format!("ref-{owner}"),
            status: ConsentStatus::Active,
            expires_at: Some(Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap()),
        },
    )
    .await
    .expect("consent semé");

    let account_id = enregistrer_compte(&comptes, &proprietaire, consent_id, owner).await;

    BalancesWriteRepository::enregistrer(
        &balances,
        NouvelleBalance {
            bank_account: account_id.clone(),
            balance_type: BalanceType::Available,
            amount_cents: 100_000,
            currency: "EUR".to_string(),
            reference_date: Utc.with_ymd_and_hms(2026, 6, 20, 0, 0, 0).unwrap(),
        },
    )
    .await
    .expect("balance semée");

    let inseree = BankTransactionsWriteRepository::enregistrer(
        &transactions,
        NouvelleTransactionBancaire {
            bank_account: account_id,
            external_transaction_id: format!("tx-{owner}"),
            status: TransactionStatus::Booked,
            label: "ACHAT".to_string(),
            amount_cents: -4_590,
            currency: "EUR".to_string(),
            booking_date: None,
            value_date: None,
        },
    )
    .await
    .expect("transaction semée");
    assert!(matches!(inseree, ResultatInsertion::Inseree(_)));
}

async fn enregistrer_compte(
    comptes: &SqlxBankAccountsWriteAdapter,
    proprietaire: &ProprietaireId,
    consent_id: ConsentId,
    owner: &str,
) -> BankAccountId {
    BankAccountsWriteRepository::enregistrer(
        comptes,
        NouveauBankAccount {
            proprietaire: proprietaire.clone(),
            consent: consent_id,
            external_account_id: format!("acct-{owner}"),
            iban: "FR7630006000011234567890189".to_string(),
            currency: "EUR".to_string(),
            next_sync_at: None,
        },
    )
    .await
    .expect("compte semé")
}

async fn compter(db: &DisposableDb, table: &str, owner: &str) -> i64 {
    let requete = match table {
        "consent" => "SELECT count(*) FROM budgy.consent WHERE owner_id = $1".to_string(),
        "bank_account" => "SELECT count(*) FROM budgy.bank_account WHERE owner_id = $1".to_string(),
        "balance" => "SELECT count(*) FROM budgy.balance b \
             JOIN budgy.bank_account a ON a.id = b.bank_account_id WHERE a.owner_id = $1"
            .to_string(),
        "bank_transaction" => "SELECT count(*) FROM budgy.bank_transaction t \
             JOIN budgy.bank_account a ON a.id = t.bank_account_id WHERE a.owner_id = $1"
            .to_string(),
        _ => panic!("table inconnue"),
    };
    sqlx::query_scalar(&requete)
        .bind(owner)
        .fetch_one(&db.pool)
        .await
        .expect("comptage")
}

fn service(
    db: &DisposableDb,
    crypto: &Arc<CryptoService>,
    source: Arc<dyn BankDataSource>,
) -> EffacementProprietaire<
    SqlxConsentsWriteAdapter,
    SqlxConsentsWriteAdapter,
    SqlxBankAccountsWriteAdapter,
    dyn BankDataSource,
> {
    let consents = SqlxConsentsWriteAdapter::new(db.pool.clone(), crypto.clone());
    let comptes = SqlxBankAccountsWriteAdapter::new(db.pool.clone(), crypto.clone());
    EffacementProprietaire::new(consents.clone(), consents, comptes, source)
}

#[tokio::test]
async fn efface_les_quatre_entites_du_sub() {
    let db = require_db!();
    let crypto = crypto();
    semer_proprietaire(&db, &crypto, OWNER_A).await;

    let source: Arc<dyn BankDataSource> = Arc::new(MockBankDataSource::new());
    let service = service(&db, &crypto, source);
    let rapport = service
        .effacer_donnees_proprietaire(ProprietaireId(OWNER_A.to_string()))
        .await
        .expect("effacement");

    assert_eq!(rapport.revocations_demandees, 1);
    assert_eq!(rapport.consentements_supprimes, 1);
    assert_eq!(compter(&db, "consent", OWNER_A).await, 0);
    assert_eq!(compter(&db, "bank_account", OWNER_A).await, 0);
    assert_eq!(compter(&db, "balance", OWNER_A).await, 0);
    assert_eq!(compter(&db, "bank_transaction", OWNER_A).await, 0);

    db.destroy().await;
}

#[tokio::test]
async fn effacement_idempotent() {
    let db = require_db!();
    let crypto = crypto();
    semer_proprietaire(&db, &crypto, OWNER_A).await;

    let source: Arc<dyn BankDataSource> = Arc::new(MockBankDataSource::new());
    let service = service(&db, &crypto, source);

    service
        .effacer_donnees_proprietaire(ProprietaireId(OWNER_A.to_string()))
        .await
        .expect("premier effacement");
    let rapport = service
        .effacer_donnees_proprietaire(ProprietaireId(OWNER_A.to_string()))
        .await
        .expect("second effacement idempotent");

    assert_eq!(rapport.consentements_supprimes, 0);
    assert_eq!(rapport.comptes_supprimes, 0);
    assert_eq!(rapport.revocations_demandees, 0);

    db.destroy().await;
}

#[tokio::test]
async fn isolation_par_sub() {
    let db = require_db!();
    let crypto = crypto();
    semer_proprietaire(&db, &crypto, OWNER_A).await;
    semer_proprietaire(&db, &crypto, OWNER_B).await;

    let source: Arc<dyn BankDataSource> = Arc::new(MockBankDataSource::new());
    let service = service(&db, &crypto, source);
    service
        .effacer_donnees_proprietaire(ProprietaireId(OWNER_A.to_string()))
        .await
        .expect("effacement A");

    assert_eq!(compter(&db, "consent", OWNER_A).await, 0);
    assert_eq!(compter(&db, "consent", OWNER_B).await, 1);
    assert_eq!(compter(&db, "bank_account", OWNER_B).await, 1);
    assert_eq!(compter(&db, "balance", OWNER_B).await, 1);
    assert_eq!(compter(&db, "bank_transaction", OWNER_B).await, 1);

    db.destroy().await;
}
