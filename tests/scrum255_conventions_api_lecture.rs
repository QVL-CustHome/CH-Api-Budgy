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
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
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
const OWNER: &str = "owner-scrum-255";
const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;

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

async fn get(db: &DisposableDb, crypto: &Arc<CryptoService>, uri: &str) -> (StatusCode, String) {
    let request = Request::builder()
        .method("GET")
        .uri(uri)
        .header("Authorization", bearer(OWNER))
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

async fn seed_consent(db: &DisposableDb, crypto: &Arc<CryptoService>) -> ConsentId {
    ConsentsWriteRepository::enregistrer(
        &SqlxConsentsWriteAdapter::new(db.pool.clone(), crypto.clone()),
        NouveauConsent {
            proprietaire: ProprietaireId(OWNER.to_string()),
            external_ref: format!("ref-{}", Uuid::new_v4()),
            status: ConsentStatus::Active,
            expires_at: Some(Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap()),
        },
    )
    .await
    .expect("consent enregistré")
}

async fn seed_account(
    db: &DisposableDb,
    crypto: &Arc<CryptoService>,
    consent: ConsentId,
    balance_cents: i64,
) -> BankAccountId {
    let compte = BankAccountsWriteRepository::enregistrer(
        &SqlxBankAccountsWriteAdapter::new(db.pool.clone(), crypto.clone()),
        NouveauBankAccount {
            proprietaire: ProprietaireId(OWNER.to_string()),
            consent,
            external_account_id: format!("acct-{}", Uuid::new_v4()),
            iban: "FR7630006000011234567890189".to_string(),
            currency: "EUR".to_string(),
            next_sync_at: None,
        },
    )
    .await
    .expect("compte enregistré");

    BalancesWriteRepository::enregistrer(
        &SqlxBalancesWriteAdapter::new(db.pool.clone(), crypto.clone()),
        NouvelleBalance {
            bank_account: compte.clone(),
            balance_type: BalanceType::Booked,
            amount_cents: balance_cents,
            currency: "EUR".to_string(),
            reference_date: Utc.with_ymd_and_hms(2026, 6, 20, 12, 0, 0).unwrap(),
        },
    )
    .await
    .expect("balance enregistrée");

    compte
}

async fn seed_transaction(
    db: &DisposableDb,
    crypto: &Arc<CryptoService>,
    compte: &BankAccountId,
    amount_cents: i64,
    booking_date: NaiveDate,
) {
    BankTransactionsWriteRepository::enregistrer(
        &SqlxBankTransactionsWriteAdapter::new(db.pool.clone(), crypto.clone()),
        NouvelleTransactionBancaire {
            bank_account: compte.clone(),
            external_transaction_id: format!("tx-{}", Uuid::new_v4()),
            status: TransactionStatus::Booked,
            label: "Achat test".to_string(),
            amount_cents,
            currency: "EUR".to_string(),
            booking_date: Some(booking_date),
            value_date: Some(booking_date),
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
async fn ac01_liste_paginee_respecte_limit_offset_par_defaut() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = seed_consent(&db, &crypto).await;
    let account = seed_account(&db, &crypto, consent, 10_000).await;
    for index in 0..60 {
        let date = jour(2026, 1, 1) + chrono::Days::new(index);
        seed_transaction(&db, &crypto, &account, 100, date).await;
    }

    let (status, corps) = get(
        &db,
        &crypto,
        &format!("/v1/accounts/{}/transactions", account.0),
    )
    .await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], json!(60));
    assert_eq!(body["data"].as_array().unwrap().len(), DEFAULT_LIMIT);

    db.destroy().await;
}

#[tokio::test]
async fn ac01_offset_decale_la_fenetre_de_pagination() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = seed_consent(&db, &crypto).await;
    let account = seed_account(&db, &crypto, consent, 0).await;
    for j in 1..=10 {
        seed_transaction(&db, &crypto, &account, 100, jour(2026, 2, j)).await;
    }

    let (status, corps) = get(
        &db,
        &crypto,
        &format!("/v1/accounts/{}/transactions?limit=5&offset=5", account.0),
    )
    .await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], json!(10));
    assert_eq!(body["data"].as_array().unwrap().len(), 5);

    db.destroy().await;
}

#[tokio::test]
async fn ac01_limit_au_maximum_est_accepte() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = seed_consent(&db, &crypto).await;
    let account = seed_account(&db, &crypto, consent, 0).await;

    let (status, _) = get(
        &db,
        &crypto,
        &format!("/v1/accounts/{}/transactions?limit={MAX_LIMIT}", account.0),
    )
    .await;

    assert_eq!(status, StatusCode::OK);

    db.destroy().await;
}

#[tokio::test]
async fn ac01_limit_zero_est_refuse_en_400() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = seed_consent(&db, &crypto).await;
    let account = seed_account(&db, &crypto, consent, 0).await;

    let (status, corps) = get(
        &db,
        &crypto,
        &format!("/v1/accounts/{}/transactions?limit=0", account.0),
    )
    .await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], json!("bad_request"));

    db.destroy().await;
}

#[tokio::test]
async fn ac01_limit_au_dessus_du_maximum_est_refuse_en_400() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = seed_consent(&db, &crypto).await;
    let account = seed_account(&db, &crypto, consent, 0).await;

    let (status, corps) = get(
        &db,
        &crypto,
        &format!(
            "/v1/accounts/{}/transactions?limit={}",
            account.0,
            MAX_LIMIT + 1
        ),
    )
    .await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], json!("bad_request"));

    db.destroy().await;
}

#[tokio::test]
async fn ac02_enveloppe_de_liste_contient_data_et_total_uniquement() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = seed_consent(&db, &crypto).await;
    seed_account(&db, &crypto, consent, 5_000).await;

    let (status, corps) = get(&db, &crypto, "/v1/accounts").await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert!(body["data"].is_array());
    assert!(body["total"].is_number());
    let keys: Vec<&String> = body.as_object().unwrap().keys().collect();
    assert_eq!(keys.len(), 2);

    db.destroy().await;
}

#[tokio::test]
async fn ac02_enveloppe_identique_sur_accounts_et_transactions() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = seed_consent(&db, &crypto).await;
    let account = seed_account(&db, &crypto, consent, 5_000).await;
    seed_transaction(&db, &crypto, &account, 250, jour(2026, 3, 1)).await;

    let (_, corps_accounts) = get(&db, &crypto, "/v1/accounts").await;
    let (_, corps_transactions) = get(
        &db,
        &crypto,
        &format!("/v1/accounts/{}/transactions", account.0),
    )
    .await;
    let accounts = body_json(&corps_accounts);
    let transactions = body_json(&corps_transactions);

    let cles_accounts: Vec<&String> = accounts.as_object().unwrap().keys().collect();
    let cles_transactions: Vec<&String> = transactions.as_object().unwrap().keys().collect();
    assert_eq!(cles_accounts, cles_transactions);

    db.destroy().await;
}

#[tokio::test]
async fn ac03_erreur_suit_le_format_code_message() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = seed_consent(&db, &crypto).await;
    let account = seed_account(&db, &crypto, consent, 0).await;

    let (status, corps) = get(
        &db,
        &crypto,
        &format!("/v1/accounts/{}/transactions?limit=0", account.0),
    )
    .await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["code"].is_string());
    assert!(body["message"].is_string());
    assert!(body.get("error").is_none());

    db.destroy().await;
}

#[tokio::test]
async fn ac03_ressource_inexistante_renvoie_404_code_not_found() {
    let db = db_or_skip!();
    let crypto = crypto();
    let inexistant = Uuid::new_v4();

    let (status, corps) = get(&db, &crypto, &format!("/v1/accounts/{inexistant}")).await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], json!("not_found"));
    assert!(body["message"].is_string());

    db.destroy().await;
}

#[tokio::test]
async fn ac03_param_invalide_renvoie_400_code_bad_request() {
    let db = db_or_skip!();
    let crypto = crypto();

    let (status, corps) = get(&db, &crypto, "/v1/accounts/pas-un-uuid").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body = body_json(&corps);
    assert_eq!(body["code"], json!("bad_request"));
    assert!(body["message"].is_string());

    db.destroy().await;
}

#[tokio::test]
async fn ac04_montants_serialises_en_entier_de_centimes() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = seed_consent(&db, &crypto).await;
    seed_account(&db, &crypto, consent, 123_456).await;

    let (status, corps) = get(&db, &crypto, "/v1/accounts").await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    let compte = &body["data"][0];
    assert_eq!(compte["balance"]["amount_cents"], json!(123_456));
    assert!(compte["balance"]["amount_cents"].is_i64());
    assert!(compte["balance"]["amount_cents"].as_str().is_none());

    db.destroy().await;
}

#[tokio::test]
async fn ac04_dates_serialisees_en_iso_8601() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = seed_consent(&db, &crypto).await;
    let account = seed_account(&db, &crypto, consent, 0).await;
    seed_transaction(&db, &crypto, &account, 999, jour(2026, 4, 15)).await;

    let (status, corps) = get(
        &db,
        &crypto,
        &format!("/v1/accounts/{}/transactions", account.0),
    )
    .await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    let transaction = &body["data"][0];
    assert_eq!(transaction["booking_date"], json!("2026-04-15"));
    assert_eq!(transaction["amount_cents"], json!(999));
    assert!(transaction["amount_cents"].is_i64());

    db.destroy().await;
}

#[tokio::test]
async fn ac05_transactions_restreintes_au_compte_demande() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = seed_consent(&db, &crypto).await;
    let compte_a = seed_account(&db, &crypto, consent.clone(), 0).await;
    let compte_b = seed_account(&db, &crypto, consent, 0).await;
    seed_transaction(&db, &crypto, &compte_a, 100, jour(2026, 5, 1)).await;
    seed_transaction(&db, &crypto, &compte_a, 200, jour(2026, 5, 2)).await;
    seed_transaction(&db, &crypto, &compte_b, 300, jour(2026, 5, 3)).await;

    let (status, corps) = get(
        &db,
        &crypto,
        &format!("/v1/accounts/{}/transactions", compte_a.0),
    )
    .await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], json!(2));

    db.destroy().await;
}

#[tokio::test]
async fn ac06_perimetre_s1_expose_comptes_solde_et_transactions() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = seed_consent(&db, &crypto).await;
    let account = seed_account(&db, &crypto, consent, 7_777).await;

    let (status_accounts, _) = get(&db, &crypto, "/v1/accounts").await;
    let (status_detail, corps_detail) =
        get(&db, &crypto, &format!("/v1/accounts/{}", account.0)).await;
    let (status_transactions, _) = get(
        &db,
        &crypto,
        &format!("/v1/accounts/{}/transactions", account.0),
    )
    .await;
    let detail = body_json(&corps_detail);

    assert_eq!(status_accounts, StatusCode::OK);
    assert_eq!(status_detail, StatusCode::OK);
    assert_eq!(status_transactions, StatusCode::OK);
    assert_eq!(detail["id"], json!(account.0.to_string()));
    assert_eq!(detail["balance"]["amount_cents"], json!(7_777));

    db.destroy().await;
}

#[tokio::test]
async fn ac06_endpoints_categories_et_budgets_absents_en_s1() {
    let db = db_or_skip!();
    let crypto = crypto();

    let (status_categories, _) = get(&db, &crypto, "/v1/categories").await;
    let (status_budgets, _) = get(&db, &crypto, "/v1/budgets").await;

    assert_eq!(status_categories, StatusCode::NOT_FOUND);
    assert_eq!(status_budgets, StatusCode::NOT_FOUND);

    db.destroy().await;
}
