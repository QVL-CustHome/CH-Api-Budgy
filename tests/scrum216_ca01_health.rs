mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use ch_api_budgy::routes::router;
use ch_api_budgy::state::AppState;
use common::DisposableDb;
use http_body_util::BodyExt;
use tower::ServiceExt;

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
    let (status, _) = get(
        AppState {
            db: db.pool.clone(),
        },
        "/health",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    db.destroy().await;
}

#[tokio::test]
async fn health_renvoie_corps_json_exact() {
    let db = require_db!();
    let (_, corps) = get(
        AppState {
            db: db.pool.clone(),
        },
        "/health",
    )
    .await;
    assert_eq!(corps, r#"{"status":"ok"}"#);
    db.destroy().await;
}
