mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use ch_api_budgy::adapters::bank::selection::{SourceBancaire, construire_source};
use ch_api_budgy::config::EnableBankingConfig;
use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::domain::consent::ConsentStatus;
use ch_api_budgy::domain::ports::bank_data_source::BankDataSource;
use ch_api_budgy::domain::ports::evenement_synchro::NoopEventPublisher;
use ch_api_budgy::domain::synchro::ParametresSynchro;
use ch_api_budgy::repository::bank_accounts::SqlxBankAccountsWriteAdapter;
use ch_api_budgy::repository::bank_transactions::SqlxBankTransactionsWriteAdapter;
use ch_api_budgy::repository::categories::SqlxCategoriesRepository;
use ch_api_budgy::repository::budgets::SqlxBudgetsRepository;
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
use ch_api_budgy::repository::regles_categorisation::SqlxReglesCategorisationRepository;
use ch_api_budgy::routes::router;
use ch_api_budgy::services::jwt::JwtService;
use ch_api_budgy::state::AppState;
use ch_api_budgy::worker::CycleSynchro;
use ch_api_budgy::worker::synchro::construire_service_synchro;
use chrono::{DateTime, TimeZone, Utc};
use common::DisposableDb;
use http_body_util::BodyExt;
use jsonwebtoken::{EncodingKey, Header};
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

const SECRET: &str = "secret_de_test_budgy_32_octets_minimum_ok!!";
const ISSUER: &str = "ch-api-authenticator";
const AUDIENCE: &str = "ch-api-budgy";
const SUB: &str = "qvl-sub-252";
const SUB_INTRUS: &str = "qvl-sub-252-intrus";
const CALLBACK_URL: &str = "https://budgy.custhome.app/banque/callback";

fn epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn jeton(sub: &str) -> String {
    let claims = json!({
        "sub": sub,
        "roles": ["budgy"],
        "iss": ISSUER,
        "aud": [AUDIENCE],
        "iat": epoch() - 10,
        "exp": epoch() + 3600,
    });
    let token = jsonwebtoken::encode(
        &Header::new(jsonwebtoken::Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(SECRET.as_bytes()),
    )
    .unwrap();
    format!("Bearer {token}")
}

fn crypto() -> Arc<CryptoService> {
    Arc::new(CryptoService::from_key(&[7u8; 32]).unwrap())
}

fn etat(db: &DisposableDb) -> AppState {
    let crypto = crypto();
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
        bank_callback_url: CALLBACK_URL.to_string(),
        db: db.pool.clone(),
        crypto,
        jwt: Arc::new(JwtService::from_secret(SECRET, ISSUER, AUDIENCE)),
    }
}

async fn appel(
    db: &DisposableDb,
    methode: &str,
    uri: &str,
    sub: &str,
    corps: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(methode)
        .uri(uri)
        .header("Authorization", jeton(sub));
    let body = match corps {
        Some(valeur) => {
            builder = builder.header("Content-Type", "application/json");
            Body::from(valeur.to_string())
        }
        None => Body::empty(),
    };
    let reponse = router(etat(db))
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = reponse.status();
    let bytes = reponse.into_body().collect().await.unwrap().to_bytes();
    let texte = String::from_utf8(bytes.to_vec()).unwrap();
    let json = if texte.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&texte).unwrap_or(Value::Null)
    };
    (status, json)
}

macro_rules! db_ou_skip {
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

async fn initier_et_activer(db: &DisposableDb, sub: &str) -> String {
    let banks = appel(db, "GET", "/v1/banks", sub, None).await;
    assert_eq!(banks.0, StatusCode::OK);
    let bank_id = banks.1["data"][0]["id"].as_str().unwrap().to_string();

    let (status, corps) = appel(
        db,
        "POST",
        "/v1/consents",
        sub,
        Some(json!({ "bank_id": bank_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let consent_id = corps["consent_id"].as_str().unwrap().to_string();

    let (status, _) = appel(
        db,
        "POST",
        "/v1/consents/callback",
        sub,
        Some(json!({ "code": "code-mock", "state": consent_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    consent_id
}

async fn expirer(db: &DisposableDb, consent_id: &str) {
    let passe = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    sqlx::query("UPDATE budgy.consent SET status = $2, expires_at = $3 WHERE id = $1")
        .bind(Uuid::parse_str(consent_id).unwrap())
        .bind(ConsentStatus::Expired.as_str())
        .bind(passe)
        .execute(&db.pool)
        .await
        .expect("forçage expiration");
}

async fn lire_consent(
    db: &DisposableDb,
    consent_id: &str,
) -> (String, Vec<u8>, Option<DateTime<Utc>>) {
    sqlx::query_as("SELECT status, external_ref, expires_at FROM budgy.consent WHERE id = $1")
        .bind(Uuid::parse_str(consent_id).unwrap())
        .fetch_one(&db.pool)
        .await
        .expect("lecture consent")
}

async fn compter_comptes(db: &DisposableDb, sub: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM budgy.bank_account WHERE owner_id = $1")
        .bind(sub)
        .fetch_one(&db.pool)
        .await
        .expect("comptage comptes")
}

async fn compter_consents(db: &DisposableDb, sub: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM budgy.consent WHERE owner_id = $1")
        .bind(sub)
        .fetch_one(&db.pool)
        .await
        .expect("comptage consents")
}

#[tokio::test]
async fn refus_409_quand_le_consent_est_a_jour_et_non_renouvelable() {
    let db = db_ou_skip!();

    let consent_id = initier_et_activer(&db, SUB).await;

    let liste = appel(&db, "GET", "/v1/consents", SUB, None).await;
    assert_eq!(liste.0, StatusCode::OK);
    assert_eq!(
        liste.1["data"][0]["renewable"],
        json!(false),
        "un consent fraichement accorde ne doit pas etre renouvelable"
    );

    let (status, corps) = appel(
        &db,
        "POST",
        &format!("/v1/consents/{consent_id}/renew"),
        SUB,
        None,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "renew d'un consent à jour doit être refusé en 409"
    );
    assert_eq!(corps["code"], json!("conflict"));

    db.destroy().await;
}

#[tokio::test]
async fn renew_d_un_consent_expire_renvoie_url_avec_state_stable() {
    let db = db_ou_skip!();

    let consent_id = initier_et_activer(&db, SUB).await;
    expirer(&db, &consent_id).await;

    let (status, corps) = appel(
        &db,
        "POST",
        &format!("/v1/consents/{consent_id}/renew"),
        SUB,
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(corps["consent_id"], json!(consent_id));
    let url = corps["authorization_url"].as_str().unwrap();
    assert!(
        url.contains(&format!("state={consent_id}")),
        "le state doit rester égal au consent_id"
    );

    db.destroy().await;
}

#[tokio::test]
async fn callback_de_renew_met_a_jour_le_meme_consent_sans_dupliquer() {
    let db = db_ou_skip!();

    let consent_id = initier_et_activer(&db, SUB).await;
    let comptes_avant = compter_comptes(&db, SUB).await;
    assert!(comptes_avant > 0);

    expirer(&db, &consent_id).await;
    let (_, ref_avant, exp_avant) = lire_consent(&db, &consent_id).await;

    let (status_renew, _) = appel(
        &db,
        "POST",
        &format!("/v1/consents/{consent_id}/renew"),
        SUB,
        None,
    )
    .await;
    assert_eq!(status_renew, StatusCode::OK);

    let (status_cb, corps_cb) = appel(
        &db,
        "POST",
        "/v1/consents/callback",
        SUB,
        Some(json!({ "code": "code-mock", "state": consent_id })),
    )
    .await;
    assert_eq!(status_cb, StatusCode::OK);
    assert_eq!(corps_cb["consent_id"], json!(consent_id));

    let (statut, ref_apres, exp_apres) = lire_consent(&db, &consent_id).await;
    assert_eq!(statut, "active");
    assert_ne!(ref_apres, ref_avant, "external_ref doit changer");
    assert_ne!(exp_apres, exp_avant, "expires_at doit changer");

    assert_eq!(
        compter_comptes(&db, SUB).await,
        comptes_avant,
        "les BankAccount ne doivent pas être dupliqués"
    );
    assert_eq!(
        compter_consents(&db, SUB).await,
        1,
        "le consent doit être réutilisé, pas dupliqué"
    );

    db.destroy().await;
}

#[tokio::test]
async fn worker_n_appelle_pas_le_fournisseur_pour_un_consent_expire() {
    let db = db_ou_skip!();

    let consent_id = initier_et_activer(&db, SUB).await;
    expirer(&db, &consent_id).await;

    let source: Arc<dyn BankDataSource> =
        construire_source(SourceBancaire::Mock, &EnableBankingConfig::default());
    let cycle = construire_service_synchro(
        db.pool.clone(),
        crypto(),
        source,
        Arc::new(NoopEventPublisher),
        ParametresSynchro::default(),
    );

    let rapport = cycle.executer_cycle().await.expect("cycle synchro");
    assert_eq!(
        rapport.comptes_synchronises, 0,
        "aucun compte d'un consent expiré ne doit être synchronisé"
    );

    let (statut, _, _) = lire_consent(&db, &consent_id).await;
    assert_eq!(statut, "expired");

    db.destroy().await;
}

#[tokio::test]
async fn anti_idor_renew_d_un_consent_d_un_autre_sub_renvoie_404() {
    let db = db_ou_skip!();

    let consent_id = initier_et_activer(&db, SUB).await;
    expirer(&db, &consent_id).await;

    let (status, corps) = appel(
        &db,
        "POST",
        &format!("/v1/consents/{consent_id}/renew"),
        SUB_INTRUS,
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(corps["code"], json!("not_found"));

    let (statut, _, _) = lire_consent(&db, &consent_id).await;
    assert_eq!(statut, "expired");

    db.destroy().await;
}
