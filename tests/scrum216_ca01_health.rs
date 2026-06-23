use axum::body::Body;
use axum::http::{Request, StatusCode};
use ch_api_budgy::routes::router;
use ch_api_budgy::state::AppState;
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn get(path: &str) -> (StatusCode, String) {
    let response = router(AppState {})
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
    let (status, _) = get("/health").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn health_renvoie_corps_json_exact() {
    let (_, corps) = get("/health").await;
    assert_eq!(corps, r#"{"status":"ok"}"#);
}
