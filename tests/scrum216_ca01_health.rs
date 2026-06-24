mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::repository::comptes::SqlxComptesRepository;
use ch_api_budgy::repository::transactions::SqlxTransactionsRepository;
use ch_api_budgy::routes::router;
use ch_api_budgy::services::jwt::JwtService;
use ch_api_budgy::state::AppState;
use common::DisposableDb;
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

const TEST_JWT_SECRET: &str = "secret-de-test-jwt-suffisamment-long-32o!";

fn test_crypto() -> Arc<CryptoService> {
    Arc::new(CryptoService::from_key(&[7u8; 32]).unwrap())
}

fn test_jwt() -> Arc<JwtService> {
    Arc::new(JwtService::from_secret(
        TEST_JWT_SECRET,
        "ch-api-authenticator",
        "ch-api-budgy",
    ))
}

fn test_state(db: &DisposableDb) -> AppState {
    AppState {
        comptes: Arc::new(SqlxComptesRepository::new(db.pool.clone())),
        transactions: Arc::new(SqlxTransactionsRepository::new(db.pool.clone())),
        db: db.pool.clone(),
        crypto: test_crypto(),
        jwt: test_jwt(),
    }
}

macro_rules! require_db {
    () => {
        match DisposableDb::create().await {
            Some(db) => db,
            None => {
                eprintln!(
                    "SCRUM-216 CA-01 ignoré : variable {} absente (Postgres jetable requis)",
                    common::ENV_ADMIN_URL
                );
                return;
            }
        }
    };
}

async fn get(state: AppState, path: &str) -> (StatusCode, String) {
    let response = router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let corps = String::from_utf8(bytes.to_vec()).unwrap();
    (status, corps)
}

#[tokio::test]
async fn health_repond_200() {
    let db = require_db!();
    let (status, _) = get(test_state(&db), "/health").await;
    assert_eq!(status, StatusCode::OK);
    db.destroy().await;
}

#[tokio::test]
async fn health_renvoie_corps_json_exact() {
    let db = require_db!();
    let (_, corps) = get(test_state(&db), "/health").await;
    assert_eq!(corps, r#"{"status":"ok"}"#);
    db.destroy().await;
}
