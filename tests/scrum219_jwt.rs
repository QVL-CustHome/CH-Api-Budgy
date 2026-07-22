use axum::body::Body;
use axum::http::{Request, StatusCode};
use ch_api_budgy::adapters::bank::selection::{SourceBancaire, construire_source};
use ch_api_budgy::config::{self, ConfigError, EnableBankingConfig};
use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::repository::bank_accounts::SqlxBankAccountsWriteAdapter;
use ch_api_budgy::repository::bank_transactions::SqlxBankTransactionsWriteAdapter;
use ch_api_budgy::repository::budgets::SqlxBudgetsRepository;
use ch_api_budgy::repository::categories::SqlxCategoriesRepository;
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
use ch_api_budgy::repository::depenses::SqlxDepensesRepository;
use ch_api_budgy::repository::regles_categorisation::SqlxReglesCategorisationRepository;
use ch_api_budgy::routes::router;
use ch_api_budgy::services::jwt::{Claims, JwtService, JwtValidationError};
use ch_api_budgy::state::AppState;
use http_body_util::BodyExt;
use jsonwebtoken::{EncodingKey, Header};
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const TEST_SECRET: &str = "secret_de_test_budgy_32_octets_minimum_ok!!";
const OTHER_SECRET: &str = "un_autre_secret_de_test_completement_distinct_42";
const EXPECTED_ISSUER: &str = "ch-api-authenticator";
const EXPECTED_AUDIENCE: &str = "ch-api-budgy";
const TEST_KEY_B64: &str = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";

static ENV_GUARD: Mutex<()> = Mutex::new(());

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn jwt_service() -> JwtService {
    JwtService::from_secret(TEST_SECRET, EXPECTED_ISSUER, EXPECTED_AUDIENCE)
}

fn sign(secret: &str, claims: &Value) -> String {
    jsonwebtoken::encode(
        &Header::new(jsonwebtoken::Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("encodage du token de test")
}

fn valid_claims(sub: &str, roles: Value) -> Value {
    json!({
        "sub": sub,
        "roles": roles,
        "iss": EXPECTED_ISSUER,
        "aud": [EXPECTED_AUDIENCE, "ch-api-other"],
        "iat": now() - 10,
        "exp": now() + 3600,
    })
}

#[test]
fn ca01_jwtservice_accepte_un_token_valide_et_restitue_sub_et_roles() {
    let token = sign(
        TEST_SECRET,
        &valid_claims("owner-123", json!(["budgy:reader", "budgy:writer"])),
    );

    let claims: Claims = jwt_service()
        .validate(&token)
        .expect("token valide accepté");

    assert_eq!(claims.sub, "owner-123");
    assert_eq!(claims.roles, vec!["budgy:reader", "budgy:writer"]);
}

#[test]
fn ca04_jwtservice_rejette_un_token_signe_avec_un_autre_secret() {
    let token = sign(OTHER_SECRET, &valid_claims("owner-123", json!([])));

    let result = jwt_service().validate(&token);

    assert!(matches!(result, Err(JwtValidationError::Invalid)));
}

#[test]
fn ca05_jwtservice_rejette_un_mauvais_issuer() {
    let mut claims = valid_claims("owner-123", json!([]));
    claims["iss"] = json!("attaquant-issuer");
    let token = sign(TEST_SECRET, &claims);

    let result = jwt_service().validate(&token);

    assert!(matches!(result, Err(JwtValidationError::Invalid)));
}

#[test]
fn ca06_jwtservice_rejette_une_audience_qui_ne_contient_pas_budgy() {
    let mut claims = valid_claims("owner-123", json!([]));
    claims["aud"] = json!(["ch-api-other", "ch-api-authenticator"]);
    let token = sign(TEST_SECRET, &claims);

    let result = jwt_service().validate(&token);

    assert!(matches!(result, Err(JwtValidationError::Invalid)));
}

#[test]
fn ca06_jwtservice_accepte_budgy_present_parmi_plusieurs_audiences() {
    let mut claims = valid_claims("owner-123", json!([]));
    claims["aud"] = json!(["ch-api-other", EXPECTED_AUDIENCE]);
    let token = sign(TEST_SECRET, &claims);

    let claims = jwt_service()
        .validate(&token)
        .expect("audience contenant budgy acceptée");

    assert_eq!(claims.sub, "owner-123");
}

#[test]
fn ca07_jwtservice_rejette_un_token_expire() {
    let mut claims = valid_claims("owner-123", json!([]));
    claims["iat"] = json!(now() - 7200);
    claims["exp"] = json!(now() - 3600);
    let token = sign(TEST_SECRET, &claims);

    let result = jwt_service().validate(&token);

    assert!(matches!(result, Err(JwtValidationError::Invalid)));
}

#[derive(Debug, PartialEq)]
enum LoadOutcome {
    Ok { jwt_len: usize },
    MissingSecret(String),
    WeakJwtSecret(usize),
    OtherError,
}

fn load_outcome(jwt_secret: Option<&str>) -> LoadOutcome {
    with_env(jwt_secret, |path| match config::load(path) {
        Ok(settings) => LoadOutcome::Ok {
            jwt_len: settings.secrets.jwt_secret.len(),
        },
        Err(ConfigError::MissingSecret(name)) => LoadOutcome::MissingSecret(name.to_string()),
        Err(ConfigError::WeakJwtSecret(len)) => LoadOutcome::WeakJwtSecret(len),
        Err(_) => LoadOutcome::OtherError,
    })
}

#[test]
fn ca08_load_echoue_si_jwt_secret_absent() {
    let _guard = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(
        load_outcome(None),
        LoadOutcome::MissingSecret("JWT_SECRET".to_string())
    );
}

#[test]
fn ca08_load_echoue_si_jwt_secret_trop_court() {
    let _guard = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let trop_court = "trop_court_31_octets_seulement!";
    assert_eq!(trop_court.len(), 31);
    assert_eq!(
        load_outcome(Some(trop_court)),
        LoadOutcome::WeakJwtSecret(31)
    );
}

#[test]
fn ca08_load_accepte_un_jwt_secret_de_32_octets() {
    let _guard = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let exactement_32 = "exactement_trente_deux_octets_32";
    assert_eq!(exactement_32.len(), 32);
    assert_eq!(
        load_outcome(Some(exactement_32)),
        LoadOutcome::Ok { jwt_len: 32 }
    );
}

fn with_env<R>(jwt_secret: Option<&str>, run: impl FnOnce(&str) -> R) -> R {
    let dir = std::env::temp_dir().join(format!("budgy_jwt_{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg_path = dir.join("config.toml");
    std::fs::write(&cfg_path, "[server]\nport = 8183\nlog_level = \"INFO\"\n").unwrap();

    let prev_db = std::env::var("DATABASE_URL").ok();
    let prev_key = std::env::var("BUDGY_ENCRYPTION_KEY").ok();
    let prev_jwt = std::env::var("JWT_SECRET").ok();

    unsafe {
        std::env::set_var("DATABASE_URL", "postgres://u:p@localhost/db");
        std::env::set_var("BUDGY_ENCRYPTION_KEY", TEST_KEY_B64);
        match jwt_secret {
            Some(v) => std::env::set_var("JWT_SECRET", v),
            None => std::env::remove_var("JWT_SECRET"),
        }
    }

    let result = run(cfg_path.to_str().unwrap());

    unsafe {
        restore("DATABASE_URL", prev_db);
        restore("BUDGY_ENCRYPTION_KEY", prev_key);
        restore("JWT_SECRET", prev_jwt);
    }
    std::fs::remove_dir_all(&dir).ok();

    result
}

unsafe fn restore(name: &str, previous: Option<String>) {
    unsafe {
        match previous {
            Some(v) => std::env::set_var(name, v),
            None => std::env::remove_var(name),
        }
    }
}

fn test_state() -> AppState {
    let db = PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .expect("pool lazy sans connexion pour la route /v1/me qui ne touche pas la base");
    let crypto = Arc::new(CryptoService::from_key(&[7u8; 32]).unwrap());
    AppState {
        consents: Arc::new(SqlxConsentsWriteAdapter::new(db.clone(), crypto.clone())),
        categories: Arc::new(SqlxCategoriesRepository::new(db.clone())),
        budgets: Arc::new(SqlxBudgetsRepository::new(db.clone())),
        depenses: Arc::new(SqlxDepensesRepository::new(db.clone(), crypto.clone())),
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

async fn get_me(state: AppState, authorization: Option<&str>) -> (StatusCode, String) {
    let mut builder = Request::builder().method("GET").uri("/v1/me");
    if let Some(value) = authorization {
        builder = builder.header("Authorization", value);
    }
    let response = router(state)
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

#[tokio::test]
async fn ca01_route_me_accepte_un_token_valide_et_renvoie_owner_id() {
    let token = sign(TEST_SECRET, &valid_claims("owner-abc", json!(["budgy"])));

    let (status, corps) = get_me(test_state(), Some(&format!("Bearer {token}"))).await;

    assert_eq!(status, StatusCode::OK);
    let body: Value = serde_json::from_str(&corps).unwrap();
    assert_eq!(body["owner_id"], json!("owner-abc"));
    assert_eq!(body["roles"], json!(["budgy"]));
}

#[tokio::test]
async fn ca02_route_me_sans_authorization_renvoie_401() {
    let (status, corps) = get_me(test_state(), None).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let body: Value = serde_json::from_str(&corps).unwrap();
    assert_eq!(body["code"], json!("unauthorized"));
    assert!(body["message"].is_string());
    assert!(body.get("error").is_none());
}

#[tokio::test]
async fn ca03_route_me_header_sans_prefixe_bearer_renvoie_401() {
    let token = sign(TEST_SECRET, &valid_claims("owner-abc", json!([])));

    let (status, _) = get_me(test_state(), Some(&token)).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ca03_route_me_header_schema_incorrect_renvoie_401() {
    let token = sign(TEST_SECRET, &valid_claims("owner-abc", json!([])));

    let (status, _) = get_me(test_state(), Some(&format!("Basic {token}"))).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ca03_route_me_header_bearer_vide_renvoie_401() {
    let (status, _) = get_me(test_state(), Some("Bearer ")).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ca04_route_me_mauvaise_signature_renvoie_401() {
    let token = sign(OTHER_SECRET, &valid_claims("owner-abc", json!([])));

    let (status, _) = get_me(test_state(), Some(&format!("Bearer {token}"))).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ca07_route_me_token_expire_renvoie_401() {
    let mut claims = valid_claims("owner-abc", json!([]));
    claims["iat"] = json!(now() - 7200);
    claims["exp"] = json!(now() - 3600);
    let token = sign(TEST_SECRET, &claims);

    let (status, _) = get_me(test_state(), Some(&format!("Bearer {token}"))).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ca09_reponse_401_ne_revele_ni_secret_ni_detail_technique() {
    let token = sign(OTHER_SECRET, &valid_claims("owner-abc", json!([])));

    let (status, corps) = get_me(test_state(), Some(&format!("Bearer {token}"))).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let bas = corps.to_lowercase();
    assert!(
        !corps.contains(TEST_SECRET),
        "le corps ne doit pas contenir le secret"
    );
    assert!(
        !corps.contains(OTHER_SECRET),
        "le corps ne doit pas contenir un secret"
    );
    assert!(
        !bas.contains("signature"),
        "pas de détail technique de signature"
    );
    assert!(!bas.contains("panic"), "pas de trace de panic");
    assert!(
        !bas.contains("decodingkey"),
        "pas de détail d'implémentation"
    );
}
