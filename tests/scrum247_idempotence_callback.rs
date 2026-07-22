mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use ch_api_budgy::adapters::bank::selection::{SourceBancaire, construire_source};
use ch_api_budgy::config::EnableBankingConfig;
use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::repository::bank_accounts::SqlxBankAccountsWriteAdapter;
use ch_api_budgy::repository::bank_transactions::SqlxBankTransactionsWriteAdapter;
use ch_api_budgy::repository::categories::SqlxCategoriesRepository;
use ch_api_budgy::repository::budgets::SqlxBudgetsRepository;
use ch_api_budgy::repository::depenses::SqlxDepensesRepository;
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
use ch_api_budgy::repository::regles_categorisation::SqlxReglesCategorisationRepository;
use ch_api_budgy::routes::router;
use ch_api_budgy::services::jwt::JwtService;
use ch_api_budgy::state::AppState;
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
const OWNER: &str = "owner-scrum-247-idempotence";
const CALLBACK_URL: &str = "https://budgy.custhome.app/banque/callback";

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
    let crypto = Arc::new(CryptoService::from_key(&[7u8; 32]).unwrap());
    AppState {
        consents: Arc::new(SqlxConsentsWriteAdapter::new(
            db.pool.clone(),
            crypto.clone(),
        )),
        categories: Arc::new(SqlxCategoriesRepository::new(db.pool.clone())),
        budgets: Arc::new(SqlxBudgetsRepository::new(db.pool.clone())),
        depenses: Arc::new(SqlxDepensesRepository::new(db.pool.clone(), crypto.clone())),
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
        bank_callback_url: CALLBACK_URL.to_string(),
        db: db.pool.clone(),
        crypto,
        jwt: Arc::new(JwtService::from_secret(TEST_SECRET, ISSUER, AUDIENCE)),
    }
}

async fn appel(
    db: &DisposableDb,
    methode: &str,
    uri: &str,
    owner: &str,
    corps: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(methode)
        .uri(uri)
        .header("Authorization", bearer(owner));
    let body = match corps {
        Some(valeur) => {
            builder = builder.header("Content-Type", "application/json");
            Body::from(valeur.to_string())
        }
        None => Body::empty(),
    };
    let response = router(state(db))
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let texte = String::from_utf8(bytes.to_vec()).unwrap();
    let json = if texte.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&texte).unwrap_or(Value::Null)
    };
    (status, json)
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

async fn statut_consent_en_base(db: &DisposableDb, consent_id: Uuid) -> String {
    sqlx::query_scalar::<_, String>("SELECT status FROM budgy.consent WHERE id = $1")
        .bind(consent_id)
        .fetch_one(&db.pool)
        .await
        .expect("lecture du statut de consentement")
}

async fn nombre_comptes_en_base(db: &DisposableDb, consent_id: Uuid, owner: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM budgy.bank_account WHERE consent_id = $1 AND owner_id = $2",
    )
    .bind(consent_id)
    .bind(owner)
    .fetch_one(&db.pool)
    .await
    .expect("comptage des comptes bancaires")
}

async fn initier(db: &DisposableDb, owner: &str) -> String {
    let banks = appel(db, "GET", "/v1/banks", owner, None).await;
    assert_eq!(banks.0, StatusCode::OK);
    let bank_id = banks.1["data"][0]["id"].as_str().unwrap().to_string();

    let (status, corps) = appel(
        db,
        "POST",
        "/v1/consents",
        owner,
        Some(json!({ "bank_id": bank_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    corps["consent_id"].as_str().unwrap().to_string()
}

async fn callback(db: &DisposableDb, owner: &str, consent_id: &str) -> (StatusCode, Value) {
    appel(
        db,
        "POST",
        "/v1/consents/callback",
        owner,
        Some(json!({ "code": "code-mock", "state": consent_id })),
    )
    .await
}

#[tokio::test]
async fn rejeu_callback_sur_consent_actif_reste_idempotent() {
    let db = db_or_skip!();

    let consent_id = initier(&db, OWNER).await;
    let consent_uuid = Uuid::parse_str(&consent_id).unwrap();

    let (status_initial, corps_initial) = callback(&db, OWNER, &consent_id).await;
    assert_eq!(status_initial, StatusCode::OK);
    assert_eq!(corps_initial["status"], json!("active"));
    let comptes_initiaux = corps_initial["comptes"].as_array().unwrap().clone();
    assert!(!comptes_initiaux.is_empty());

    let comptes_apres_premier = nombre_comptes_en_base(&db, consent_uuid, OWNER).await;
    assert_eq!(comptes_apres_premier as usize, comptes_initiaux.len());

    let (status_rejeu, corps_rejeu) = callback(&db, OWNER, &consent_id).await;

    assert_eq!(status_rejeu, StatusCode::OK);
    assert_eq!(corps_rejeu["status"], json!("active"));
    assert_eq!(corps_rejeu["consent_id"], json!(consent_id));

    let comptes_rejoues = corps_rejeu["comptes"].as_array().unwrap();
    assert_eq!(comptes_rejoues.len(), comptes_initiaux.len());

    let ids_initiaux: Vec<&str> = comptes_initiaux
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    let ids_rejoues: Vec<&str> = comptes_rejoues
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids_initiaux, ids_rejoues);

    let comptes_apres_rejeu = nombre_comptes_en_base(&db, consent_uuid, OWNER).await;
    assert_eq!(comptes_apres_rejeu, comptes_apres_premier);

    let statut = statut_consent_en_base(&db, consent_uuid).await;
    assert_eq!(statut, "active");

    db.destroy().await;
}
