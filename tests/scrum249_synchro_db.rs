mod common;

use std::sync::Arc;

use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::domain::bank_account::{BankAccountId, NouveauBankAccount, PlanificationSynchro};
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::{ConsentId, ConsentStatus, NouveauConsent};
use ch_api_budgy::domain::ports::ecriture::{
    BankAccountsWriteRepository, BankTransactionsWriteRepository, ConsentsWriteRepository,
    PlanificationSynchroWriteRepository, ResultatInsertion,
};
use ch_api_budgy::domain::ports::lecture::PlanificationSynchroReadRepository;
use ch_api_budgy::domain::transaction_bancaire::{NouvelleTransactionBancaire, TransactionStatus};
use ch_api_budgy::repository::bank_accounts::{
    SqlxBankAccountsRepository, SqlxBankAccountsWriteAdapter,
};
use ch_api_budgy::repository::bank_transactions::SqlxBankTransactionsWriteAdapter;
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
use chrono::{DateTime, Duration, TimeZone, Utc};
use common::DisposableDb;

const QUOTA: i32 = 4;

macro_rules! require_db {
    () => {
        match DisposableDb::create().await {
            Some(db) => {
                db.migrate().await;
                db
            }
            None => {
                eprintln!(
                    "SCRUM-249 synchro DB ignoré : variable {} absente (Postgres jetable requis)",
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

fn maintenant() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 25, 9, 0, 0).unwrap()
}

async fn consent(
    db: &DisposableDb,
    crypto: &Arc<CryptoService>,
    owner: &str,
    statut: ConsentStatus,
    expire_le: Option<DateTime<Utc>>,
) -> ConsentId {
    ConsentsWriteRepository::enregistrer(
        &SqlxConsentsWriteAdapter::new(db.pool.clone(), crypto.clone()),
        NouveauConsent {
            proprietaire: ProprietaireId(owner.to_string()),
            external_ref: format!("ref-{owner}"),
            status: statut,
            expires_at: expire_le,
        },
    )
    .await
    .expect("consent enregistré")
}

async fn compte(
    db: &DisposableDb,
    crypto: &Arc<CryptoService>,
    owner: &str,
    consent: ConsentId,
    next_sync_at: Option<DateTime<Utc>>,
) -> BankAccountId {
    BankAccountsWriteRepository::enregistrer(
        &SqlxBankAccountsWriteAdapter::new(db.pool.clone(), crypto.clone()),
        NouveauBankAccount {
            proprietaire: ProprietaireId(owner.to_string()),
            consent,
            external_account_id: format!("acct-{owner}"),
            iban: "FR7630006000011234567890189".to_string(),
            currency: "EUR".to_string(),
            next_sync_at,
        },
    )
    .await
    .expect("compte enregistré")
}

#[tokio::test]
async fn la_selection_ne_retient_que_les_comptes_echeants_actifs_sous_quota() {
    let db = require_db!();
    let crypto = crypto();
    let now = maintenant();

    let actif_echu = consent(
        &db,
        &crypto,
        "echu",
        ConsentStatus::Active,
        Some(now + Duration::days(30)),
    )
    .await;
    let echu = compte(
        &db,
        &crypto,
        "echu",
        actif_echu,
        Some(now - Duration::hours(1)),
    )
    .await;

    let actif_futur = consent(
        &db,
        &crypto,
        "futur",
        ConsentStatus::Active,
        Some(now + Duration::days(30)),
    )
    .await;
    let _non_echu = compte(
        &db,
        &crypto,
        "futur",
        actif_futur,
        Some(now + Duration::hours(3)),
    )
    .await;

    let expire = consent(
        &db,
        &crypto,
        "expire",
        ConsentStatus::Active,
        Some(now - Duration::hours(1)),
    )
    .await;
    let _compte_expire = compte(
        &db,
        &crypto,
        "expire",
        expire,
        Some(now - Duration::hours(1)),
    )
    .await;

    let lecteur = SqlxBankAccountsWriteAdapter::new(db.pool.clone(), crypto.clone());
    let echeants = lecteur
        .lister_comptes_echeants(now, QUOTA)
        .await
        .expect("sélection data-driven");

    assert_eq!(echeants.len(), 1);
    assert_eq!(echeants[0].compte.id, echu);

    db.destroy().await;
}

#[tokio::test]
async fn reserver_creneau_respecte_le_quota_journalier() {
    let db = require_db!();
    let crypto = crypto();
    let now = maintenant();
    let jour = now.date_naive();

    let consent_id = consent(
        &db,
        &crypto,
        "quota",
        ConsentStatus::Active,
        Some(now + Duration::days(30)),
    )
    .await;
    let compte_id = compte(
        &db,
        &crypto,
        "quota",
        consent_id,
        Some(now - Duration::hours(1)),
    )
    .await;

    let ecriture = SqlxBankAccountsWriteAdapter::new(db.pool.clone(), crypto.clone());

    for attendu in 1..=QUOTA {
        let plan = PlanificationSynchro {
            next_sync_at: now,
            last_sync_day: jour,
            last_sync_at: now,
        };
        let reserve = ecriture
            .reserver_creneau(&compte_id, plan, QUOTA)
            .await
            .expect("réservation");
        assert!(reserve, "le créneau {attendu} doit être réservé");
    }

    let plan_excedentaire = PlanificationSynchro {
        next_sync_at: now,
        last_sync_day: jour,
        last_sync_at: now,
    };
    let refus = ecriture
        .reserver_creneau(&compte_id, plan_excedentaire, QUOTA)
        .await
        .expect("réservation refusée");
    assert!(!refus, "au-delà du quota la réservation doit échouer");

    let lecteur = SqlxBankAccountsRepository::new(db.pool.clone());
    let relu = lecteur
        .fetch(&crypto, &compte_id)
        .await
        .expect("lecture compte")
        .expect("compte présent");
    assert_eq!(relu.sync_count_today, QUOTA);

    db.destroy().await;
}

#[tokio::test]
async fn reserver_creneau_reinitialise_le_compteur_au_changement_de_jour() {
    let db = require_db!();
    let crypto = crypto();
    let now = maintenant();

    let consent_id = consent(
        &db,
        &crypto,
        "reset",
        ConsentStatus::Active,
        Some(now + Duration::days(30)),
    )
    .await;
    let compte_id = compte(
        &db,
        &crypto,
        "reset",
        consent_id,
        Some(now - Duration::hours(1)),
    )
    .await;

    let ecriture = SqlxBankAccountsWriteAdapter::new(db.pool.clone(), crypto.clone());

    for _ in 0..QUOTA {
        assert!(
            ecriture
                .reserver_creneau(
                    &compte_id,
                    PlanificationSynchro {
                        next_sync_at: now,
                        last_sync_day: now.date_naive(),
                        last_sync_at: now,
                    },
                    QUOTA,
                )
                .await
                .expect("réservation jour 1")
        );
    }

    let demain = now + Duration::days(1);
    let plan_jour2 = PlanificationSynchro {
        next_sync_at: demain,
        last_sync_day: demain.date_naive(),
        last_sync_at: demain,
    };
    assert!(
        ecriture
            .reserver_creneau(&compte_id, plan_jour2, QUOTA)
            .await
            .expect("réservation jour 2 après reset"),
        "le changement de jour réinitialise le quota"
    );

    let lecteur = SqlxBankAccountsRepository::new(db.pool.clone());
    let relu = lecteur
        .fetch(&crypto, &compte_id)
        .await
        .expect("lecture")
        .expect("présent");
    assert_eq!(relu.sync_count_today, 1);

    db.destroy().await;
}

#[tokio::test]
async fn reserver_creneau_ne_depasse_pas_le_quota_sous_concurrence() {
    let db = require_db!();
    let crypto = crypto();
    let now = maintenant();
    let jour = now.date_naive();

    let consent_id = consent(
        &db,
        &crypto,
        "concurrence",
        ConsentStatus::Active,
        Some(now + Duration::days(30)),
    )
    .await;
    let compte_id = compte(
        &db,
        &crypto,
        "concurrence",
        consent_id,
        Some(now - Duration::hours(1)),
    )
    .await;

    let tentatives = QUOTA * 5;
    let mut handles = Vec::with_capacity(tentatives as usize);
    for _ in 0..tentatives {
        let ecriture = SqlxBankAccountsWriteAdapter::new(db.pool.clone(), crypto.clone());
        let compte = compte_id.clone();
        handles.push(tokio::spawn(async move {
            ecriture
                .reserver_creneau(
                    &compte,
                    PlanificationSynchro {
                        next_sync_at: now,
                        last_sync_day: jour,
                        last_sync_at: now,
                    },
                    QUOTA,
                )
                .await
                .expect("réservation concurrente")
        }));
    }

    let mut reservations_reussies = 0;
    for handle in handles {
        if handle.await.expect("tâche de réservation") {
            reservations_reussies += 1;
        }
    }
    assert_eq!(
        reservations_reussies, QUOTA,
        "le quota borne le nombre de réservations"
    );

    let lecteur = SqlxBankAccountsRepository::new(db.pool.clone());
    let relu = lecteur
        .fetch(&crypto, &compte_id)
        .await
        .expect("lecture compte")
        .expect("compte présent");
    assert!(
        relu.sync_count_today <= QUOTA,
        "l'invariant de quota doit tenir sous concurrence"
    );
    assert_eq!(relu.sync_count_today, QUOTA);

    db.destroy().await;
}

#[tokio::test]
async fn une_transaction_pending_devient_booked_sans_doublon() {
    let db = require_db!();
    let crypto = crypto();
    let now = maintenant();

    let consent_id = consent(
        &db,
        &crypto,
        "tx",
        ConsentStatus::Active,
        Some(now + Duration::days(30)),
    )
    .await;
    let compte_id = compte(
        &db,
        &crypto,
        "tx",
        consent_id,
        Some(now - Duration::hours(1)),
    )
    .await;

    let ecriture = SqlxBankTransactionsWriteAdapter::new(db.pool.clone(), crypto.clone());

    let pending = NouvelleTransactionBancaire {
        bank_account: compte_id.clone(),
        external_transaction_id: "tx-achat".to_string(),
        status: TransactionStatus::Pending,
        label: "CARTE ACHAT".to_string(),
        amount_cents: -4_590,
        currency: "EUR".to_string(),
        booking_date: None,
        value_date: None,
    };
    let inseree = ecriture
        .enregistrer(pending)
        .await
        .expect("insertion pending");
    assert!(matches!(inseree, ResultatInsertion::Inseree(_)));

    let booked = NouvelleTransactionBancaire {
        bank_account: compte_id.clone(),
        external_transaction_id: "tx-achat".to_string(),
        status: TransactionStatus::Booked,
        label: "CARTE ACHAT".to_string(),
        amount_cents: -4_590,
        currency: "EUR".to_string(),
        booking_date: Some(now.date_naive()),
        value_date: Some(now.date_naive()),
    };
    let rejeu = ecriture.enregistrer(booked).await.expect("rejeu booked");
    assert_eq!(rejeu, ResultatInsertion::Doublon);

    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM budgy.bank_transaction WHERE bank_account_id = $1",
    )
    .bind(compte_id.0)
    .fetch_one(&db.pool)
    .await
    .expect("comptage");
    assert_eq!(total, 1, "aucun doublon ne doit subsister");

    let statut: String =
        sqlx::query_scalar("SELECT status FROM budgy.bank_transaction WHERE bank_account_id = $1")
            .bind(compte_id.0)
            .fetch_one(&db.pool)
            .await
            .expect("lecture statut");
    assert_eq!(statut, TransactionStatus::Booked.as_str());

    db.destroy().await;
}
