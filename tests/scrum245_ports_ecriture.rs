mod common;

use std::sync::Arc;

use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::domain::balance::{BalanceType, NouvelleBalance};
use ch_api_budgy::domain::bank_account::{BankAccountId, NouveauBankAccount};
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::{ConsentId, ConsentStatus, NouveauConsent};
use ch_api_budgy::domain::ports::ecriture::{
    BalancesWriteRepository, BankAccountsWriteRepository, BankTransactionsWriteRepository,
    ConsentsWriteRepository, ResultatInsertion,
};
use ch_api_budgy::domain::transaction_bancaire::{NouvelleTransactionBancaire, TransactionStatus};
use ch_api_budgy::repository::balances::{SqlxBalancesRepository, SqlxBalancesWriteAdapter};
use ch_api_budgy::repository::bank_accounts::{
    SqlxBankAccountsRepository, SqlxBankAccountsWriteAdapter,
};
use ch_api_budgy::repository::bank_transactions::{
    SqlxBankTransactionsRepository, SqlxBankTransactionsWriteAdapter,
};
use ch_api_budgy::repository::consents::{SqlxConsentsRepository, SqlxConsentsWriteAdapter};
use chrono::{TimeZone, Utc};
use common::DisposableDb;

const OWNER: &str = "owner-scrum-245-ports";
const IBAN_CLAIR: &str = "FR7630006000011234567890189";
const LABEL_CLAIR: &str = "VIREMENT SALAIRE ACME SARL";
const EXTERNAL_REF_CLAIR: &str = "consent-ref-ports-1a2b";
const EXTERNAL_ACCOUNT_CLAIR: &str = "acct-ext-ports-9d4e";
const EXTERNAL_TX_CLAIR: &str = "tx-ext-ports-77ce";
const MONTANT: i64 = 245_001;

macro_rules! require_db {
    () => {
        match DisposableDb::create().await {
            Some(db) => {
                db.migrate().await;
                db
            }
            None => {
                eprintln!(
                    "SCRUM-245 ports d'écriture ignoré : variable {} absente (Postgres jetable requis)",
                    common::ENV_ADMIN_URL
                );
                return;
            }
        }
    };
}

fn crypto() -> Arc<CryptoService> {
    Arc::new(CryptoService::from_key(&[9u8; 32]).expect("clé de test 32 octets valide"))
}

fn proprietaire() -> ProprietaireId {
    ProprietaireId(OWNER.to_string())
}

fn nouveau_consent() -> NouveauConsent {
    NouveauConsent {
        proprietaire: proprietaire(),
        external_ref: EXTERNAL_REF_CLAIR.to_string(),
        status: ConsentStatus::Active,
        expires_at: Some(Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap()),
    }
}

fn nouveau_compte(consent: ConsentId) -> NouveauBankAccount {
    NouveauBankAccount {
        proprietaire: proprietaire(),
        consent,
        external_account_id: EXTERNAL_ACCOUNT_CLAIR.to_string(),
        iban: IBAN_CLAIR.to_string(),
        currency: "EUR".to_string(),
        next_sync_at: Some(Utc.with_ymd_and_hms(2026, 7, 1, 8, 0, 0).unwrap()),
    }
}

fn nouvelle_balance(compte: BankAccountId) -> NouvelleBalance {
    NouvelleBalance {
        bank_account: compte,
        balance_type: BalanceType::Available,
        amount_cents: MONTANT,
        currency: "EUR".to_string(),
        reference_date: Utc.with_ymd_and_hms(2026, 6, 20, 0, 0, 0).unwrap(),
    }
}

fn nouvelle_transaction(compte: &BankAccountId) -> NouvelleTransactionBancaire {
    NouvelleTransactionBancaire {
        bank_account: compte.clone(),
        external_transaction_id: EXTERNAL_TX_CLAIR.to_string(),
        status: TransactionStatus::Booked,
        label: LABEL_CLAIR.to_string(),
        amount_cents: MONTANT,
        currency: "EUR".to_string(),
        booking_date: None,
        value_date: None,
    }
}

#[tokio::test]
async fn le_port_consents_enregistre_et_relit() {
    let db = require_db!();
    let crypto = crypto();
    let adapter = SqlxConsentsWriteAdapter::new(db.pool.clone(), crypto.clone());

    let id = ConsentsWriteRepository::enregistrer(&adapter, nouveau_consent())
        .await
        .expect("le port consents enregistre");

    let lecteur = SqlxConsentsRepository::new(db.pool.clone());
    let relu = lecteur
        .fetch(&crypto, &id)
        .await
        .expect("lecture consent")
        .expect("consent présent");

    assert_eq!(relu.external_ref, EXTERNAL_REF_CLAIR);
    assert_eq!(relu.status, ConsentStatus::Active);

    db.destroy().await;
}

#[tokio::test]
async fn le_port_bank_accounts_enregistre_et_relit() {
    let db = require_db!();
    let crypto = crypto();
    let consent_id = ConsentsWriteRepository::enregistrer(
        &SqlxConsentsWriteAdapter::new(db.pool.clone(), crypto.clone()),
        nouveau_consent(),
    )
    .await
    .expect("consent prérequis");

    let adapter = SqlxBankAccountsWriteAdapter::new(db.pool.clone(), crypto.clone());
    let id = BankAccountsWriteRepository::enregistrer(&adapter, nouveau_compte(consent_id))
        .await
        .expect("le port bank_accounts enregistre");

    let lecteur = SqlxBankAccountsRepository::new(db.pool.clone());
    let relu = lecteur
        .fetch(&crypto, &id)
        .await
        .expect("lecture compte")
        .expect("compte présent");

    assert_eq!(relu.external_account_id, EXTERNAL_ACCOUNT_CLAIR);
    assert!(relu.iban_masked.ends_with("0189"));

    db.destroy().await;
}

#[tokio::test]
async fn le_port_balances_enregistre_et_relit() {
    let db = require_db!();
    let crypto = crypto();
    let consent_id = ConsentsWriteRepository::enregistrer(
        &SqlxConsentsWriteAdapter::new(db.pool.clone(), crypto.clone()),
        nouveau_consent(),
    )
    .await
    .expect("consent prérequis");
    let account_id = BankAccountsWriteRepository::enregistrer(
        &SqlxBankAccountsWriteAdapter::new(db.pool.clone(), crypto.clone()),
        nouveau_compte(consent_id),
    )
    .await
    .expect("compte prérequis");

    let adapter = SqlxBalancesWriteAdapter::new(db.pool.clone(), crypto.clone());
    let id = BalancesWriteRepository::enregistrer(&adapter, nouvelle_balance(account_id))
        .await
        .expect("le port balances enregistre");

    let lecteur = SqlxBalancesRepository::new(db.pool.clone());
    let relu = lecteur
        .fetch(&crypto, &id)
        .await
        .expect("lecture balance")
        .expect("balance présente");

    assert_eq!(relu.amount_cents, MONTANT);
    assert_eq!(relu.balance_type, BalanceType::Available);

    db.destroy().await;
}

#[tokio::test]
async fn le_port_bank_transactions_enregistre_puis_signale_le_doublon_au_rejeu() {
    let db = require_db!();
    let crypto = crypto();
    let consent_id = ConsentsWriteRepository::enregistrer(
        &SqlxConsentsWriteAdapter::new(db.pool.clone(), crypto.clone()),
        nouveau_consent(),
    )
    .await
    .expect("consent prérequis");
    let account_id = BankAccountsWriteRepository::enregistrer(
        &SqlxBankAccountsWriteAdapter::new(db.pool.clone(), crypto.clone()),
        nouveau_compte(consent_id),
    )
    .await
    .expect("compte prérequis");

    let adapter = SqlxBankTransactionsWriteAdapter::new(db.pool.clone(), crypto.clone());

    let premier =
        BankTransactionsWriteRepository::enregistrer(&adapter, nouvelle_transaction(&account_id))
            .await
            .expect("le port bank_transactions enregistre");
    let id = match premier {
        ResultatInsertion::Inseree(id) => id,
        ResultatInsertion::Doublon => panic!("première insertion ne doit pas être un doublon"),
    };

    let rejeu =
        BankTransactionsWriteRepository::enregistrer(&adapter, nouvelle_transaction(&account_id))
            .await
            .expect("le port bank_transactions rejoue le lot");
    assert_eq!(
        rejeu,
        ResultatInsertion::Doublon,
        "le rejeu du même lot doit renvoyer Doublon via le port"
    );

    let lecteur = SqlxBankTransactionsRepository::new(db.pool.clone());
    let relu = lecteur
        .fetch(&crypto, &id)
        .await
        .expect("lecture transaction")
        .expect("transaction présente");
    assert_eq!(relu.external_transaction_id, EXTERNAL_TX_CLAIR);
    assert_eq!(relu.label, LABEL_CLAIR);

    let total: i64 =
        sqlx::query_scalar("SELECT count(*) FROM budgy.bank_transaction WHERE bank_account_id = $1")
            .bind(account_id.0)
            .fetch_one(&db.pool)
            .await
            .expect("comptage transactions");
    assert_eq!(total, 1, "aucun doublon ne doit subsister après le rejeu");

    db.destroy().await;
}
