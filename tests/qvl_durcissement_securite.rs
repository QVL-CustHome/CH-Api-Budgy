use axum::body::Body;
use axum::http::{Request, StatusCode};
use ch_api_budgy::adapters::bank::selection::{SourceBancaire, construire_source};
use ch_api_budgy::config::EnableBankingConfig;
use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::repository::bank_accounts::SqlxBankAccountsWriteAdapter;
use ch_api_budgy::repository::bank_transactions::SqlxBankTransactionsWriteAdapter;
use ch_api_budgy::repository::categories::SqlxCategoriesRepository;
use ch_api_budgy::repository::budgets::SqlxBudgetsRepository;
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
use ch_api_budgy::repository::regles_categorisation::SqlxReglesCategorisationRepository;
use ch_api_budgy::routes::router;
use ch_api_budgy::services::jwt::JwtService;
use ch_api_budgy::state::AppState;
use jsonwebtoken::{EncodingKey, Header};
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tower::ServiceExt;

const TEST_SECRET: &str = "secret_de_test_budgy_32_octets_minimum_ok!!";
const EXPECTED_ISSUER: &str = "ch-api-authenticator";
const EXPECTED_AUDIENCE: &str = "ch-api-budgy";

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn jwt_service() -> JwtService {
    JwtService::from_secret(TEST_SECRET, EXPECTED_ISSUER, EXPECTED_AUDIENCE)
}

fn test_state() -> AppState {
    let db = PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .expect("pool lazy sans connexion");
    let crypto = Arc::new(CryptoService::from_key(&[7u8; 32]).unwrap());
    AppState {
        consents: Arc::new(SqlxConsentsWriteAdapter::new(db.clone(), crypto.clone())),
        categories: Arc::new(SqlxCategoriesRepository::new(db.clone())),
        budgets: Arc::new(SqlxBudgetsRepository::new(db.clone())),
        regles_categorisation: Arc::new(SqlxReglesCategorisationRepository::new(db.clone())),
        bank_accounts: Arc::new(SqlxBankAccountsWriteAdapter::new(
            db.clone(),
            crypto.clone(),
        )),
        bank_transactions: Arc::new(SqlxBankTransactionsWriteAdapter::new(
            db.clone(),
            crypto.clone(),
        )),
        bank_source: construire_source(SourceBancaire::Mock, &EnableBankingConfig::default()),
        bank_callback_url: "https://budgy.custhome.app/banque/callback".to_string(),
        db,
        crypto,
        jwt: Arc::new(jwt_service()),
    }
}

fn sign_with_roles(roles: Value) -> String {
    let claims = json!({
        "sub": "owner-securite",
        "roles": roles,
        "iss": EXPECTED_ISSUER,
        "aud": [EXPECTED_AUDIENCE],
        "iat": now() - 10,
        "exp": now() + 3600,
    });
    jsonwebtoken::encode(
        &Header::new(jsonwebtoken::Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(TEST_SECRET.as_bytes()),
    )
    .expect("encodage du token de test")
}

async fn get_me(authorization: &str) -> StatusCode {
    let request = Request::builder()
        .method("GET")
        .uri("/v1/me")
        .header("Authorization", authorization)
        .body(Body::empty())
        .unwrap();
    router(test_state())
        .oneshot(request)
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn f2_token_valide_avec_role_budgy_est_accepte() {
    let token = sign_with_roles(json!(["budgy"]));

    let status = get_me(&format!("Bearer {token}")).await;

    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn f2_token_valide_avec_role_budgy_parmi_dautres_est_accepte() {
    let token = sign_with_roles(json!(["compta", "budgy", "admin"]));

    let status = get_me(&format!("Bearer {token}")).await;

    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn f2_token_valide_sans_role_budgy_est_refuse_403() {
    let token = sign_with_roles(json!(["compta", "admin"]));

    let status = get_me(&format!("Bearer {token}")).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn f2_token_valide_avec_roles_vides_est_refuse_403() {
    let token = sign_with_roles(json!([]));

    let status = get_me(&format!("Bearer {token}")).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn f02_token_de_taille_normale_n_est_pas_impacte_par_la_borne() {
    let token = sign_with_roles(json!(["budgy"]));
    assert!(token.len() < 1024);

    let status = get_me(&format!("Bearer {token}")).await;

    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn f02_token_juste_sous_le_seuil_est_transmis_au_parsing() {
    let bourrage: String = "A".repeat(2500);
    let token = sign_with_roles(json!(["budgy", bourrage]));
    assert!(token.len() > 3000, "token = {} octets", token.len());
    assert!(token.len() < 4096, "token = {} octets", token.len());

    let status = get_me(&format!("Bearer {token}")).await;

    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn f02_sonde_du_seuil_de_taille() {
    let tailles_bourrage = [0usize, 1000, 2000, 3000, 4000, 5000, 6000, 7000, 8000];
    let mut observations = Vec::new();
    for n in tailles_bourrage {
        let token = sign_with_roles(json!(["budgy", "A".repeat(n)]));
        let header = format!("Bearer {token}");
        let header_len = header.len();
        let status = get_me(&header).await;
        observations.push((header_len, status));
    }
    for (len, status) in &observations {
        println!("authorization_len={len} -> {status}");
    }
    let premier_rejet = observations.iter().find(|(_, s)| *s != StatusCode::OK);
    assert!(
        premier_rejet.is_some(),
        "aucune borne de taille observée jusqu'à 8000 octets de bourrage"
    );
}

#[tokio::test]
async fn f02_token_largement_au_dessus_du_seuil_est_rejete_401_avant_parsing() {
    let token = "B".repeat(64 * 1024);

    let status = get_me(&format!("Bearer {token}")).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn f02_token_enorme_meme_signe_valablement_est_rejete_401() {
    let bourrage: String = "C".repeat(64 * 1024);
    let token = sign_with_roles(json!(["budgy", bourrage]));
    assert!(token.len() > 32 * 1024, "token = {} octets", token.len());

    let status = get_me(&format!("Bearer {token}")).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
