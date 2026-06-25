mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use ch_api_budgy::adapters::bank::selection::{SourceBancaire, construire_source};
use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::repository::comptes::SqlxComptesRepository;
use ch_api_budgy::repository::transactions::SqlxTransactionsRepository;
use ch_api_budgy::routes::router;
use ch_api_budgy::services::jwt::JwtService;
use ch_api_budgy::state::AppState;
use common::DisposableDb;
use http_body_util::BodyExt;
use jsonwebtoken::{EncodingKey, Header};
use serde_json::{Value, json};
use sqlx::Executor;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

const TEST_SECRET: &str = "secret_de_test_budgy_32_octets_minimum_ok!!";
const ISSUER: &str = "ch-api-authenticator";
const AUDIENCE: &str = "ch-api-budgy";
const OWNER: &str = "owner-scrum-255";
const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;

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

fn state(db: &DisposableDb) -> AppState {
    AppState {
        comptes: Arc::new(SqlxComptesRepository::new(db.pool.clone())),
        transactions: Arc::new(SqlxTransactionsRepository::new(db.pool.clone())),
        bank_source: construire_source(SourceBancaire::Mock),
        db: db.pool.clone(),
        crypto: Arc::new(CryptoService::from_key(&[7u8; 32]).unwrap()),
        jwt: Arc::new(JwtService::from_secret(TEST_SECRET, ISSUER, AUDIENCE)),
    }
}

async fn get(db: &DisposableDb, uri: &str) -> (StatusCode, String) {
    let request = Request::builder()
        .method("GET")
        .uri(uri)
        .header("Authorization", bearer(OWNER))
        .body(Body::empty())
        .unwrap();
    let response = router(state(db)).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

fn body_json(corps: &str) -> Value {
    serde_json::from_str(corps)
        .unwrap_or_else(|_| panic!("le corps n'est pas un JSON valide : {corps}"))
}

async fn seed_account(db: &DisposableDb, label: &str, balance_cents: i64) -> Uuid {
    let id = Uuid::new_v4();
    db.pool
        .execute(
            sqlx::query(
                "INSERT INTO budgy.account (id, owner_id, label, institution, iban, currency, balance_cents) \
                 VALUES ($1, $2, $3, 'Banque Test', 'FR7612345678901234567890123', 'EUR', $4)",
            )
            .bind(id)
            .bind(OWNER)
            .bind(label)
            .bind(balance_cents),
        )
        .await
        .expect("insertion compte");
    id
}

async fn seed_transaction(
    db: &DisposableDb,
    account_id: Uuid,
    amount_cents: i64,
    operation_date: &str,
) -> Uuid {
    let id = Uuid::new_v4();
    db.pool
        .execute(
            sqlx::query(
                "INSERT INTO budgy.transaction \
                 (id, account_id, owner_id, label, amount_cents, direction, currency, operation_date) \
                 VALUES ($1, $2, $3, 'Achat test', $4, 'debit', 'EUR', $5::date)",
            )
            .bind(id)
            .bind(account_id)
            .bind(OWNER)
            .bind(amount_cents)
            .bind(operation_date),
        )
        .await
        .expect("insertion transaction");
    id
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
    let account = seed_account(&db, "Compte courant", 10_000).await;
    for index in 0..60 {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap() + chrono::Days::new(index);
        seed_transaction(&db, account, 100, &date.to_string()).await;
    }

    let (status, corps) = get(&db, "/v1/transactions").await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], json!(60));
    assert_eq!(body["data"].as_array().unwrap().len(), DEFAULT_LIMIT);

    db.destroy().await;
}

#[tokio::test]
async fn ac01_offset_decale_la_fenetre_de_pagination() {
    let db = db_or_skip!();
    let account = seed_account(&db, "Compte courant", 0).await;
    for jour in 1..=10 {
        seed_transaction(&db, account, 100, &format!("2026-02-{jour:02}")).await;
    }

    let (status, corps) = get(&db, "/v1/transactions?limit=5&offset=5").await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], json!(10));
    assert_eq!(body["data"].as_array().unwrap().len(), 5);

    db.destroy().await;
}

#[tokio::test]
async fn ac01_limit_au_maximum_est_accepte() {
    let db = db_or_skip!();
    seed_account(&db, "Compte courant", 0).await;

    let (status, _) = get(&db, &format!("/v1/transactions?limit={MAX_LIMIT}")).await;

    assert_eq!(status, StatusCode::OK);

    db.destroy().await;
}

#[tokio::test]
async fn ac01_limit_zero_est_refuse_en_400() {
    let db = db_or_skip!();
    seed_account(&db, "Compte courant", 0).await;

    let (status, corps) = get(&db, "/v1/transactions?limit=0").await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], json!("bad_request"));

    db.destroy().await;
}

#[tokio::test]
async fn ac01_limit_au_dessus_du_maximum_est_refuse_en_400() {
    let db = db_or_skip!();
    seed_account(&db, "Compte courant", 0).await;

    let (status, corps) = get(&db, &format!("/v1/transactions?limit={}", MAX_LIMIT + 1)).await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], json!("bad_request"));

    db.destroy().await;
}

#[tokio::test]
async fn ac02_enveloppe_de_liste_contient_data_et_total_uniquement() {
    let db = db_or_skip!();
    seed_account(&db, "Compte courant", 5_000).await;

    let (status, corps) = get(&db, "/v1/accounts").await;
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
    let account = seed_account(&db, "Compte courant", 5_000).await;
    seed_transaction(&db, account, 250, "2026-03-01").await;

    let (_, corps_accounts) = get(&db, "/v1/accounts").await;
    let (_, corps_transactions) = get(&db, "/v1/transactions").await;
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
    seed_account(&db, "Compte courant", 0).await;

    let (status, corps) = get(&db, "/v1/transactions?limit=0").await;
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
    let inexistant = Uuid::new_v4();

    let (status, corps) = get(&db, &format!("/v1/accounts/{inexistant}/balance")).await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], json!("not_found"));
    assert!(body["message"].is_string());

    db.destroy().await;
}

#[tokio::test]
async fn ac03_param_invalide_renvoie_400_code_bad_request() {
    let db = db_or_skip!();
    seed_account(&db, "Compte courant", 0).await;

    let (status, corps) = get(&db, "/v1/accounts/pas-un-uuid/balance").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body = body_json(&corps);
    assert_eq!(body["code"], json!("bad_request"));
    assert!(body["message"].is_string());

    db.destroy().await;
}

#[tokio::test]
async fn ac04_montants_serialises_en_entier_de_centimes() {
    let db = db_or_skip!();
    seed_account(&db, "Compte courant", 123_456).await;

    let (status, corps) = get(&db, "/v1/accounts").await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    let compte = &body["data"][0];
    assert_eq!(compte["balance_cents"], json!(123_456));
    assert!(compte["balance_cents"].is_i64());
    assert!(compte["balance_cents"].as_str().is_none());
    assert!(compte["balance_cents"].as_f64().map(|v| v.fract() == 0.0).unwrap_or(true));

    db.destroy().await;
}

#[tokio::test]
async fn ac04_dates_serialisees_en_iso_8601() {
    let db = db_or_skip!();
    let account = seed_account(&db, "Compte courant", 0).await;
    seed_transaction(&db, account, 999, "2026-04-15").await;

    let (status, corps) = get(&db, "/v1/transactions").await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    let transaction = &body["data"][0];
    assert_eq!(transaction["operation_date"], json!("2026-04-15"));
    assert_eq!(transaction["amount_cents"], json!(999));
    assert!(transaction["amount_cents"].is_i64());
    let created_at = transaction["created_at"].as_str().unwrap();
    assert!(chrono::DateTime::parse_from_rfc3339(created_at).is_ok());

    db.destroy().await;
}

#[tokio::test]
async fn ac05_filtre_account_id_restreint_les_transactions() {
    let db = db_or_skip!();
    let compte_a = seed_account(&db, "Compte A", 0).await;
    let compte_b = seed_account(&db, "Compte B", 0).await;
    seed_transaction(&db, compte_a, 100, "2026-05-01").await;
    seed_transaction(&db, compte_a, 200, "2026-05-02").await;
    seed_transaction(&db, compte_b, 300, "2026-05-03").await;

    let (status, corps) = get(&db, &format!("/v1/transactions?account_id={compte_a}")).await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], json!(2));

    db.destroy().await;
}

#[tokio::test]
async fn ac05_filtre_from_to_borne_les_dates() {
    let db = db_or_skip!();
    let account = seed_account(&db, "Compte courant", 0).await;
    seed_transaction(&db, account, 100, "2026-06-01").await;
    seed_transaction(&db, account, 200, "2026-06-10").await;
    seed_transaction(&db, account, 300, "2026-06-20").await;

    let (status, corps) = get(&db, "/v1/transactions?from=2026-06-05&to=2026-06-15").await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], json!(1));

    db.destroy().await;
}

#[tokio::test]
async fn ac05_from_posterieur_a_to_est_refuse_en_400() {
    let db = db_or_skip!();
    seed_account(&db, "Compte courant", 0).await;

    let (status, corps) = get(&db, "/v1/transactions?from=2026-06-20&to=2026-06-01").await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], json!("bad_request"));

    db.destroy().await;
}

#[tokio::test]
async fn ac05_date_de_filtre_non_iso_est_refusee_en_400() {
    let db = db_or_skip!();
    seed_account(&db, "Compte courant", 0).await;

    let (status, corps) = get(&db, "/v1/transactions?from=15-06-2026").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body = body_json(&corps);
    assert_eq!(body["code"], json!("bad_request"));
    assert!(body["message"].is_string());

    db.destroy().await;
}

#[tokio::test]
async fn ac06_perimetre_s1_expose_comptes_solde_et_transactions() {
    let db = db_or_skip!();
    let account = seed_account(&db, "Compte courant", 7_777).await;

    let (status_accounts, _) = get(&db, "/v1/accounts").await;
    let (status_balance, corps_balance) = get(&db, &format!("/v1/accounts/{account}/balance")).await;
    let (status_transactions, _) = get(&db, "/v1/transactions").await;
    let balance = body_json(&corps_balance);

    assert_eq!(status_accounts, StatusCode::OK);
    assert_eq!(status_balance, StatusCode::OK);
    assert_eq!(status_transactions, StatusCode::OK);
    assert_eq!(balance["account_id"], json!(account.to_string()));
    assert_eq!(balance["balance_cents"], json!(7_777));

    db.destroy().await;
}

#[tokio::test]
async fn ac06_endpoints_categories_et_budgets_absents_en_s1() {
    let db = db_or_skip!();

    let (status_categories, _) = get(&db, "/v1/categories").await;
    let (status_budgets, _) = get(&db, "/v1/budgets").await;

    assert_eq!(status_categories, StatusCode::NOT_FOUND);
    assert_eq!(status_budgets, StatusCode::NOT_FOUND);

    db.destroy().await;
}
