mod common;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use ch_api_budgy::adapters::bank::selection::{SourceBancaire, construire_source};
use ch_api_budgy::config::EnableBankingConfig;
use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::domain::balance::Balance;
use ch_api_budgy::domain::bank_account::BankAccount;
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::{Consent, ConsentStatus};
use ch_api_budgy::domain::ports::bank_data_source::{
    BankDataSource, BankDataSourceError, ConsentementInitie, DemandeConsentement, Etablissement,
    ReponseAutorisation,
};
use ch_api_budgy::domain::ports::evenement_synchro::NoopEventPublisher;
use ch_api_budgy::domain::synchro::ParametresSynchro;
use ch_api_budgy::domain::transaction_bancaire::TransactionBancaire;
use ch_api_budgy::repository::bank_accounts::SqlxBankAccountsWriteAdapter;
use ch_api_budgy::repository::bank_transactions::SqlxBankTransactionsWriteAdapter;
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
use ch_api_budgy::routes::router;
use ch_api_budgy::services::jwt::JwtService;
use ch_api_budgy::state::AppState;
use ch_api_budgy::worker::CycleSynchro;
use ch_api_budgy::worker::synchro::construire_service_synchro;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use common::DisposableDb;
use http_body_util::BodyExt;
use jsonwebtoken::{EncodingKey, Header};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tower::ServiceExt;
use uuid::Uuid;

const TEST_SECRET: &str = "secret_de_test_budgy_32_octets_minimum_ok!!";
const ISSUER: &str = "ch-api-authenticator";
const AUDIENCE: &str = "ch-api-budgy";
const OWNER: &str = "owner-scrum-252";
const AUTRE_OWNER: &str = "owner-scrum-252-autre";
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

fn crypto() -> Arc<CryptoService> {
    Arc::new(CryptoService::from_key(&[7u8; 32]).unwrap())
}

fn state(db: &DisposableDb) -> AppState {
    let crypto = crypto();
    AppState {
        consents: Arc::new(SqlxConsentsWriteAdapter::new(
            db.pool.clone(),
            crypto.clone(),
        )),
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

async fn etablir_consent_actif(db: &DisposableDb, owner: &str) -> String {
    let consent_id = initier(db, owner).await;
    let (status, _) = callback(db, owner, &consent_id).await;
    assert_eq!(status, StatusCode::OK);
    consent_id
}

async fn forcer_expiration(db: &DisposableDb, consent_id: &str) {
    let date_passee = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    sqlx::query("UPDATE budgy.consent SET status = $2, expires_at = $3 WHERE id = $1")
        .bind(Uuid::parse_str(consent_id).unwrap())
        .bind(ConsentStatus::Expired.as_str())
        .bind(date_passee)
        .execute(&db.pool)
        .await
        .expect("forçage de l'expiration");
}

async fn etat_consent(
    db: &DisposableDb,
    consent_id: &str,
) -> (String, Vec<u8>, Option<DateTime<Utc>>) {
    sqlx::query_as::<_, (String, Vec<u8>, Option<DateTime<Utc>>)>(
        "SELECT status, external_ref, expires_at FROM budgy.consent WHERE id = $1",
    )
    .bind(Uuid::parse_str(consent_id).unwrap())
    .fetch_one(&db.pool)
    .await
    .expect("lecture de l'état du consentement")
}

async fn nombre_comptes(db: &DisposableDb, consent_id: &str, owner: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM budgy.bank_account WHERE consent_id = $1 AND owner_id = $2",
    )
    .bind(Uuid::parse_str(consent_id).unwrap())
    .bind(owner)
    .fetch_one(&db.pool)
    .await
    .expect("comptage des comptes")
}

#[tokio::test]
async fn ac2_renew_d_un_consent_expire_renvoie_une_url_avec_state_egal_au_consent_id() {
    let db = db_or_skip!();

    let consent_id = etablir_consent_actif(&db, OWNER).await;
    forcer_expiration(&db, &consent_id).await;

    let (status, corps) = appel(
        &db,
        "POST",
        &format!("/v1/consents/{consent_id}/renew"),
        OWNER,
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(corps["consent_id"], json!(consent_id));
    let url = corps["authorization_url"].as_str().unwrap();
    assert!(
        url.contains(&format!("state={consent_id}")),
        "le state de la redirection doit être le consent_id stable"
    );

    let (statut, _, _) = etat_consent(&db, &consent_id).await;
    assert_eq!(statut, "pending");

    db.destroy().await;
}

#[tokio::test]
async fn ac4_callback_de_renew_met_a_jour_le_meme_consent_sans_dupliquer_les_comptes() {
    let db = db_or_skip!();

    let consent_id = etablir_consent_actif(&db, OWNER).await;
    let comptes_avant = nombre_comptes(&db, &consent_id, OWNER).await;
    assert!(comptes_avant > 0);

    forcer_expiration(&db, &consent_id).await;
    let (_, ref_expire, expire_at_expire) = etat_consent(&db, &consent_id).await;

    let (status_renew, _) = appel(
        &db,
        "POST",
        &format!("/v1/consents/{consent_id}/renew"),
        OWNER,
        None,
    )
    .await;
    assert_eq!(status_renew, StatusCode::OK);

    let (status_callback, corps_callback) = callback(&db, OWNER, &consent_id).await;
    assert_eq!(status_callback, StatusCode::OK);
    assert_eq!(corps_callback["consent_id"], json!(consent_id));
    assert_eq!(corps_callback["status"], json!("active"));

    let (statut, ref_apres, expire_at_apres) = etat_consent(&db, &consent_id).await;
    assert_eq!(statut, "active");
    assert_ne!(ref_apres, ref_expire, "external_ref doit être mis à jour");
    assert_ne!(
        expire_at_apres, expire_at_expire,
        "expires_at doit être mis à jour"
    );

    let comptes_apres = nombre_comptes(&db, &consent_id, OWNER).await;
    assert_eq!(
        comptes_apres, comptes_avant,
        "le re-consentement ne doit pas dupliquer les comptes"
    );

    let total_consents: i64 =
        sqlx::query_scalar("SELECT count(*) FROM budgy.consent WHERE owner_id = $1")
            .bind(OWNER)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(total_consents, 1, "aucun nouveau consent ne doit être créé");

    db.destroy().await;
}

#[tokio::test]
async fn anti_idor_renew_d_un_consent_d_un_autre_sub_renvoie_404() {
    let db = db_or_skip!();

    let consent_id = etablir_consent_actif(&db, OWNER).await;
    forcer_expiration(&db, &consent_id).await;

    let (status, corps) = appel(
        &db,
        "POST",
        &format!("/v1/consents/{consent_id}/renew"),
        AUTRE_OWNER,
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(corps["code"], json!("not_found"));

    let (statut, _, _) = etat_consent(&db, &consent_id).await;
    assert_eq!(statut, "expired");

    db.destroy().await;
}

#[tokio::test]
async fn renew_refuse_un_consent_a_jour() {
    let db = db_or_skip!();

    let consent_id = etablir_consent_actif(&db, OWNER).await;

    let (status, corps) = appel(
        &db,
        "POST",
        &format!("/v1/consents/{consent_id}/renew"),
        OWNER,
        None,
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(corps["code"], json!("conflict"));

    db.destroy().await;
}

#[tokio::test]
async fn liste_des_consents_expose_le_statut_renouvelable() {
    let db = db_or_skip!();

    let consent_id = etablir_consent_actif(&db, OWNER).await;
    forcer_expiration(&db, &consent_id).await;

    let (status, corps) = appel(&db, "GET", "/v1/consents", OWNER, None).await;
    assert_eq!(status, StatusCode::OK);

    let consent = &corps["data"][0];
    assert_eq!(consent["renewal"], json!("expired"));
    assert_eq!(consent["renewable"], json!(true));

    db.destroy().await;
}

#[derive(Default)]
struct SourceCompteur {
    appels: AtomicU32,
}

#[async_trait]
impl BankDataSource for SourceCompteur {
    async fn lister_etablissements(&self) -> Result<Vec<Etablissement>, BankDataSourceError> {
        self.appels.fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
    }

    async fn initier_consentement(
        &self,
        _demande: DemandeConsentement,
    ) -> Result<ConsentementInitie, BankDataSourceError> {
        self.appels.fetch_add(1, Ordering::SeqCst);
        Err(BankDataSourceError::SourceNonConfiguree)
    }

    async fn completer_consentement(
        &self,
        _proprietaire: &ProprietaireId,
        _reponse: ReponseAutorisation,
    ) -> Result<Consent, BankDataSourceError> {
        self.appels.fetch_add(1, Ordering::SeqCst);
        Err(BankDataSourceError::SourceNonConfiguree)
    }

    async fn lister_comptes(
        &self,
        _consent: &Consent,
    ) -> Result<Vec<BankAccount>, BankDataSourceError> {
        self.appels.fetch_add(1, Ordering::SeqCst);
        Err(BankDataSourceError::SourceNonConfiguree)
    }

    async fn solde(
        &self,
        _consent: &Consent,
        _compte: &BankAccount,
    ) -> Result<Vec<Balance>, BankDataSourceError> {
        self.appels.fetch_add(1, Ordering::SeqCst);
        Err(BankDataSourceError::SourceNonConfiguree)
    }

    async fn lister_transactions(
        &self,
        _consent: &Consent,
        _compte: &BankAccount,
        _depuis: NaiveDate,
    ) -> Result<Vec<TransactionBancaire>, BankDataSourceError> {
        self.appels.fetch_add(1, Ordering::SeqCst);
        Err(BankDataSourceError::SourceNonConfiguree)
    }

    async fn revoquer_consentement(
        &self,
        _consent: &Consent,
    ) -> Result<Consent, BankDataSourceError> {
        self.appels.fetch_add(1, Ordering::SeqCst);
        Err(BankDataSourceError::SourceNonConfiguree)
    }
}

#[tokio::test]
async fn ac3_le_worker_n_appelle_pas_le_fournisseur_pour_un_consent_expire() {
    let db = db_or_skip!();

    let consent_id = etablir_consent_actif(&db, OWNER).await;
    forcer_expiration(&db, &consent_id).await;

    let source = Arc::new(SourceCompteur::default());
    let bank_source: Arc<dyn BankDataSource> = source.clone();
    let cycle = construire_service_synchro(
        db.pool.clone(),
        crypto(),
        bank_source,
        Arc::new(NoopEventPublisher),
        ParametresSynchro::default(),
    );

    let rapport = cycle.executer_cycle().await.expect("cycle de synchro");

    assert_eq!(
        source.appels.load(Ordering::SeqCst),
        0,
        "aucun appel fournisseur ne doit avoir lieu pour un consent expiré"
    );
    assert_eq!(rapport.comptes_synchronises, 0);

    let (statut, _, _) = etat_consent(&db, &consent_id).await;
    assert_eq!(statut, "expired");

    db.destroy().await;
}
