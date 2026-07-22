mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use ch_api_budgy::adapters::bank::selection::{SourceBancaire, construire_source};
use ch_api_budgy::config::EnableBankingConfig;
use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::domain::balance::{BalanceType, NouvelleBalance};
use ch_api_budgy::domain::bank_account::{BankAccountId, NouveauBankAccount};
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::{ConsentId, ConsentStatus, NouveauConsent};
use ch_api_budgy::domain::ports::ecriture::{
    BalancesWriteRepository, BankAccountsWriteRepository, BankTransactionsWriteRepository,
    ConsentsWriteRepository,
};
use ch_api_budgy::domain::transaction_bancaire::{NouvelleTransactionBancaire, TransactionStatus};
use ch_api_budgy::repository::balances::SqlxBalancesWriteAdapter;
use ch_api_budgy::repository::bank_accounts::SqlxBankAccountsWriteAdapter;
use ch_api_budgy::repository::bank_transactions::SqlxBankTransactionsWriteAdapter;
use ch_api_budgy::repository::categories::SqlxCategoriesRepository;
use ch_api_budgy::repository::budgets::SqlxBudgetsRepository;
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
use ch_api_budgy::repository::regles_categorisation::SqlxReglesCategorisationRepository;
use ch_api_budgy::routes::router;
use ch_api_budgy::services::jwt::JwtService;
use ch_api_budgy::state::AppState;
use chrono::{NaiveDate, TimeZone, Utc};
use common::DisposableDb;
use http_body_util::BodyExt;
use jsonwebtoken::{EncodingKey, Header};
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

const TEST_SECRET: &str = "secret_de_test_budgy_32_octets_minimum_ok!!";
const ISSUER: &str = "ch-api-authenticator";
const AUDIENCE: &str = "ch-api-budgy";
const OWNER: &str = "owner-scrum-248";
const AUTRE_OWNER: &str = "owner-scrum-248-intrus";

fn crypto() -> Arc<CryptoService> {
    Arc::new(CryptoService::from_key(&[7u8; 32]).expect("clé de test 32 octets valide"))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn bearer(owner: &str) -> String {
    let claims = json!({
        "sub": owner,
        "roles": ["budgy"],
        "iss": ISSUER,
        "aud": [AUDIENCE],
        "iat": now() - 10,
        "exp": now() + 3600,
    });
    let token = jsonwebtoken::encode(
        &Header::new(jsonwebtoken::Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(TEST_SECRET.as_bytes()),
    )
    .unwrap();
    format!("Bearer {token}")
}

fn state(db: &DisposableDb, crypto: &Arc<CryptoService>) -> AppState {
    AppState {
        consents: Arc::new(SqlxConsentsWriteAdapter::new(
            db.pool.clone(),
            crypto.clone(),
        )),
        categories: Arc::new(SqlxCategoriesRepository::new(db.pool.clone())),
        budgets: Arc::new(SqlxBudgetsRepository::new(db.pool.clone())),
        regles_categorisation: Arc::new(SqlxReglesCategorisationRepository::new(db.pool.clone())),
        bank_accounts: Arc::new(SqlxBankAccountsWriteAdapter::new(
            db.pool.clone(),
            crypto.clone(),
        )),
        bank_transactions: Arc::new(SqlxBankTransactionsWriteAdapter::new(
            db.pool.clone(),
            crypto.clone(),
        )),
        bank_source: construire_source(SourceBancaire::Mock, &EnableBankingConfig::default()),
        bank_callback_url: "https://budgy.custhome.app/banque/callback".to_string(),
        db: db.pool.clone(),
        crypto: crypto.clone(),
        jwt: Arc::new(JwtService::from_secret(TEST_SECRET, ISSUER, AUDIENCE)),
    }
}

async fn get(
    db: &DisposableDb,
    crypto: &Arc<CryptoService>,
    owner: &str,
    uri: &str,
) -> (StatusCode, String) {
    let request = Request::builder()
        .method("GET")
        .uri(uri)
        .header("Authorization", bearer(owner))
        .body(Body::empty())
        .unwrap();
    let response = router(state(db, crypto)).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

fn body_json(corps: &str) -> Value {
    serde_json::from_str(corps)
        .unwrap_or_else(|_| panic!("le corps n'est pas un JSON valide : {corps}"))
}

async fn consent(db: &DisposableDb, crypto: &Arc<CryptoService>, owner: &str) -> ConsentId {
    ConsentsWriteRepository::enregistrer(
        &SqlxConsentsWriteAdapter::new(db.pool.clone(), crypto.clone()),
        NouveauConsent {
            proprietaire: ProprietaireId(owner.to_string()),
            external_ref: format!("ref-{owner}-{}", Uuid::new_v4()),
            status: ConsentStatus::Active,
            expires_at: Some(Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap()),
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
    iban: &str,
) -> BankAccountId {
    BankAccountsWriteRepository::enregistrer(
        &SqlxBankAccountsWriteAdapter::new(db.pool.clone(), crypto.clone()),
        NouveauBankAccount {
            proprietaire: ProprietaireId(owner.to_string()),
            consent,
            external_account_id: format!("acct-{}", Uuid::new_v4()),
            iban: iban.to_string(),
            currency: "EUR".to_string(),
            next_sync_at: None,
        },
    )
    .await
    .expect("compte enregistré")
}

async fn balance(
    db: &DisposableDb,
    crypto: &Arc<CryptoService>,
    compte: &BankAccountId,
    balance_type: BalanceType,
    amount_cents: i64,
) {
    BalancesWriteRepository::enregistrer(
        &SqlxBalancesWriteAdapter::new(db.pool.clone(), crypto.clone()),
        NouvelleBalance {
            bank_account: compte.clone(),
            balance_type,
            amount_cents,
            currency: "EUR".to_string(),
            reference_date: Utc.with_ymd_and_hms(2026, 6, 20, 12, 0, 0).unwrap(),
        },
    )
    .await
    .expect("balance enregistrée");
}

struct SeedTransaction<'a> {
    db: &'a DisposableDb,
    crypto: &'a Arc<CryptoService>,
    compte: &'a BankAccountId,
    external_id: &'a str,
    label: &'a str,
    amount_cents: i64,
    status: TransactionStatus,
    booking_date: Option<NaiveDate>,
}

async fn transaction(seed: SeedTransaction<'_>) {
    BankTransactionsWriteRepository::enregistrer(
        &SqlxBankTransactionsWriteAdapter::new(seed.db.pool.clone(), seed.crypto.clone()),
        NouvelleTransactionBancaire {
            bank_account: seed.compte.clone(),
            external_transaction_id: seed.external_id.to_string(),
            status: seed.status,
            label: seed.label.to_string(),
            amount_cents: seed.amount_cents,
            currency: "EUR".to_string(),
            booking_date: seed.booking_date,
            value_date: seed.booking_date,
        },
    )
    .await
    .expect("transaction enregistrée");
}

fn jour(annee: i32, mois: u32, jour: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(annee, mois, jour).unwrap()
}

macro_rules! db_or_skip {
    () => {
        match DisposableDb::create().await {
            Some(db) => {
                db.migrate().await;
                db
            }
            None => {
                eprintln!("BUDGY_TEST_DATABASE_URL absente : test ignoré");
                return;
            }
        }
    };
}

#[tokio::test]
async fn liste_comptes_expose_solde_dechiffre_et_iban_masque() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = consent(&db, &crypto, OWNER).await;
    let compte = compte(&db, &crypto, OWNER, consent, "FR7630006000011234567890189").await;
    balance(&db, &crypto, &compte, BalanceType::Booked, 15_327).await;

    let (status, corps) = get(&db, &crypto, OWNER, "/v1/accounts").await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], json!(1));
    let item = &body["data"][0];
    assert_eq!(item["id"], json!(compte.0.to_string()));
    assert_eq!(item["currency"], json!("EUR"));
    assert_eq!(item["iban_masked"], json!("***********************0189"));
    assert!(!item["iban_masked"].as_str().unwrap().contains("FR76"));
    assert!(item.get("iban").is_none());
    assert_eq!(item["balance"]["amount_cents"], json!(15_327));
    assert!(item["balance"]["amount_cents"].is_i64());
    assert_eq!(item["balance"]["type"], json!("booked"));
    assert!(item["balance"]["at"].is_string());

    db.destroy().await;
}

#[tokio::test]
async fn liste_comptes_selectionne_la_balance_booked_de_preference() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = consent(&db, &crypto, OWNER).await;
    let compte = compte(&db, &crypto, OWNER, consent, "FR7630006000011234567890189").await;
    balance(&db, &crypto, &compte, BalanceType::Available, 90_000).await;
    balance(&db, &crypto, &compte, BalanceType::Booked, 80_000).await;

    let (status, corps) = get(&db, &crypto, OWNER, "/v1/accounts").await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    let balance = &body["data"][0]["balance"];
    assert_eq!(balance["type"], json!("booked"));
    assert_eq!(balance["amount_cents"], json!(80_000));

    db.destroy().await;
}

#[tokio::test]
async fn liste_comptes_ne_renvoie_que_ceux_du_sub() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent_owner = consent(&db, &crypto, OWNER).await;
    compte(
        &db,
        &crypto,
        OWNER,
        consent_owner,
        "FR7630006000011234567890189",
    )
    .await;
    let consent_intrus = consent(&db, &crypto, AUTRE_OWNER).await;
    compte(
        &db,
        &crypto,
        AUTRE_OWNER,
        consent_intrus,
        "FR7610107001011234567890129",
    )
    .await;

    let (status, corps) = get(&db, &crypto, OWNER, "/v1/accounts").await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], json!(1));
    assert_eq!(body["data"].as_array().unwrap().len(), 1);

    db.destroy().await;
}

#[tokio::test]
async fn liste_transactions_dechiffrees_paginees_et_triees_par_date_desc() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = consent(&db, &crypto, OWNER).await;
    let compte = compte(&db, &crypto, OWNER, consent, "FR7630006000011234567890189").await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-1",
        label: "ANCIEN",
        amount_cents: -100,
        status: TransactionStatus::Booked,
        booking_date: Some(jour(2026, 1, 1)),
    })
    .await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-2",
        label: "MILIEU",
        amount_cents: -200,
        status: TransactionStatus::Booked,
        booking_date: Some(jour(2026, 3, 15)),
    })
    .await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-3",
        label: "RECENT",
        amount_cents: -300,
        status: TransactionStatus::Booked,
        booking_date: Some(jour(2026, 6, 1)),
    })
    .await;

    let (status, corps) = get(
        &db,
        &crypto,
        OWNER,
        &format!("/v1/accounts/{}/transactions?limit=2&offset=0", compte.0),
    )
    .await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], json!(3));
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    assert_eq!(data[0]["label"], json!("RECENT"));
    assert_eq!(data[0]["amount_cents"], json!(-300));
    assert!(data[0]["amount_cents"].is_i64());
    assert_eq!(data[0]["booking_date"], json!("2026-06-01"));
    assert_eq!(data[1]["label"], json!("MILIEU"));

    let (_, corps_page2) = get(
        &db,
        &crypto,
        OWNER,
        &format!("/v1/accounts/{}/transactions?limit=2&offset=2", compte.0),
    )
    .await;
    let page2 = body_json(&corps_page2);
    let data2 = page2["data"].as_array().unwrap();
    assert_eq!(data2.len(), 1);
    assert_eq!(data2[0]["label"], json!("ANCIEN"));

    db.destroy().await;
}

#[tokio::test]
async fn transactions_d_un_compte_d_un_autre_sub_renvoie_404() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent_intrus = consent(&db, &crypto, AUTRE_OWNER).await;
    let compte_intrus = compte(
        &db,
        &crypto,
        AUTRE_OWNER,
        consent_intrus,
        "FR7610107001011234567890129",
    )
    .await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte_intrus,
        external_id: "tx-intrus",
        label: "SECRET",
        amount_cents: -999,
        status: TransactionStatus::Booked,
        booking_date: Some(jour(2026, 5, 5)),
    })
    .await;

    let (status, corps) = get(
        &db,
        &crypto,
        OWNER,
        &format!("/v1/accounts/{}/transactions", compte_intrus.0),
    )
    .await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], json!("not_found"));
    assert!(!corps.contains("SECRET"));

    db.destroy().await;
}

#[tokio::test]
async fn detail_compte_d_un_autre_sub_renvoie_404() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent_intrus = consent(&db, &crypto, AUTRE_OWNER).await;
    let compte_intrus = compte(
        &db,
        &crypto,
        AUTRE_OWNER,
        consent_intrus,
        "FR7610107001011234567890129",
    )
    .await;

    let (status, corps) = get(
        &db,
        &crypto,
        OWNER,
        &format!("/v1/accounts/{}", compte_intrus.0),
    )
    .await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], json!("not_found"));

    db.destroy().await;
}

#[tokio::test]
async fn pending_devenue_booked_reste_une_seule_ligne_a_l_etat_courant() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = consent(&db, &crypto, OWNER).await;
    let compte = compte(&db, &crypto, OWNER, consent, "FR7630006000011234567890189").await;

    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-evolutive",
        label: "CARTE ACHAT",
        amount_cents: -4_590,
        status: TransactionStatus::Pending,
        booking_date: None,
    })
    .await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-evolutive",
        label: "CARTE ACHAT",
        amount_cents: -4_590,
        status: TransactionStatus::Booked,
        booking_date: Some(jour(2026, 6, 25)),
    })
    .await;

    let (status, corps) = get(
        &db,
        &crypto,
        OWNER,
        &format!("/v1/accounts/{}/transactions", compte.0),
    )
    .await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], json!(1));
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["status"], json!("booked"));
    assert_eq!(data[0]["label"], json!("CARTE ACHAT"));
    assert_eq!(data[0]["amount_cents"], json!(-4_590));

    db.destroy().await;
}
