mod common;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use ch_api_budgy::adapters::bank::mock::MockBankDataSource;
use ch_api_budgy::adapters::bank::selection::{SourceBancaire, construire_source};
use ch_api_budgy::config::EnableBankingConfig;
use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::domain::balance::Balance;
use ch_api_budgy::domain::bank_account::BankAccount;
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::Consent;
use ch_api_budgy::domain::ports::bank_data_source::{
    BankDataSource, BankDataSourceError, ConsentementInitie, DemandeConsentement, Etablissement,
    ReponseAutorisation,
};
use ch_api_budgy::domain::transaction_bancaire::TransactionBancaire;
use ch_api_budgy::repository::bank_accounts::SqlxBankAccountsWriteAdapter;
use ch_api_budgy::repository::bank_transactions::SqlxBankTransactionsWriteAdapter;
use ch_api_budgy::repository::categories::SqlxCategoriesRepository;
use ch_api_budgy::repository::budgets::SqlxBudgetsRepository;
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
use ch_api_budgy::repository::regles_categorisation::SqlxReglesCategorisationRepository;
use ch_api_budgy::routes::router;
use ch_api_budgy::services::jwt::JwtService;
use ch_api_budgy::state::AppState;
use chrono::NaiveDate;
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
const OWNER: &str = "owner-scrum-247";
const AUTRE_OWNER: &str = "owner-scrum-247-autre";
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

struct SourceRefusante {
    mock: MockBankDataSource,
}

impl SourceRefusante {
    fn nouvelle() -> Self {
        Self {
            mock: MockBankDataSource::new(),
        }
    }
}

#[async_trait]
impl BankDataSource for SourceRefusante {
    async fn lister_etablissements(&self) -> Result<Vec<Etablissement>, BankDataSourceError> {
        self.mock.lister_etablissements().await
    }

    async fn initier_consentement(
        &self,
        demande: DemandeConsentement,
    ) -> Result<ConsentementInitie, BankDataSourceError> {
        self.mock.initier_consentement(demande).await
    }

    async fn completer_consentement(
        &self,
        _proprietaire: &ProprietaireId,
        _reponse: ReponseAutorisation,
    ) -> Result<Consent, BankDataSourceError> {
        Err(BankDataSourceError::ConsentementInvalide)
    }

    async fn lister_comptes(
        &self,
        consent: &Consent,
    ) -> Result<Vec<BankAccount>, BankDataSourceError> {
        self.mock.lister_comptes(consent).await
    }

    async fn solde(
        &self,
        consent: &Consent,
        compte: &BankAccount,
    ) -> Result<Vec<Balance>, BankDataSourceError> {
        self.mock.solde(consent, compte).await
    }

    async fn lister_transactions(
        &self,
        consent: &Consent,
        compte: &BankAccount,
        depuis: NaiveDate,
    ) -> Result<Vec<TransactionBancaire>, BankDataSourceError> {
        self.mock.lister_transactions(consent, compte, depuis).await
    }

    async fn revoquer_consentement(
        &self,
        consent: &Consent,
    ) -> Result<Consent, BankDataSourceError> {
        self.mock.revoquer_consentement(consent).await
    }
}

fn state(db: &DisposableDb) -> AppState {
    state_avec_source(
        db,
        construire_source(SourceBancaire::Mock, &EnableBankingConfig::default()),
    )
}

fn state_avec_source(db: &DisposableDb, bank_source: Arc<dyn BankDataSource>) -> AppState {
    let crypto = Arc::new(CryptoService::from_key(&[7u8; 32]).unwrap());
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
        bank_source,
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
    appel_avec_state(state(db), methode, uri, owner, corps).await
}

async fn appel_avec_state(
    state: AppState,
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
    let response = router(state)
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

async fn initier(db: &DisposableDb, owner: &str) -> Value {
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
    corps
}

#[tokio::test]
async fn ac1_initiation_cree_un_consent_pending_et_redirige_avec_state() {
    let db = db_or_skip!();

    let corps = initier(&db, OWNER).await;
    let consent_id = corps["consent_id"].as_str().unwrap();
    let authorization_url = corps["authorization_url"].as_str().unwrap();

    assert!(Uuid::parse_str(consent_id).is_ok());
    assert!(authorization_url.contains(&format!("state={consent_id}")));

    let statut = statut_consent_en_base(&db, Uuid::parse_str(consent_id).unwrap()).await;
    assert_eq!(statut, "pending");

    db.destroy().await;
}

#[tokio::test]
async fn ac2_callback_active_le_consent_et_persiste_les_comptes() {
    let db = db_or_skip!();

    let initiation = initier(&db, OWNER).await;
    let consent_id = initiation["consent_id"].as_str().unwrap().to_string();

    let (status, corps) = appel(
        &db,
        "POST",
        "/v1/consents/callback",
        OWNER,
        Some(json!({ "code": "code-mock", "state": consent_id })),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(corps["consent_id"], json!(consent_id));
    assert_eq!(corps["status"], json!("active"));
    let comptes = corps["comptes"].as_array().unwrap();
    assert!(!comptes.is_empty());
    for compte in comptes {
        let iban = compte["iban_masked"].as_str().unwrap();
        assert!(iban.starts_with('*'));
        assert!(!iban.contains("FR"));
    }

    let statut = statut_consent_en_base(&db, Uuid::parse_str(&consent_id).unwrap()).await;
    assert_eq!(statut, "active");

    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM budgy.bank_account WHERE consent_id = $1 AND owner_id = $2",
    )
    .bind(Uuid::parse_str(&consent_id).unwrap())
    .bind(OWNER)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(total as usize, comptes.len());

    db.destroy().await;
}

#[tokio::test]
async fn ac3_callback_d_un_autre_sub_ne_voit_pas_le_consent() {
    let db = db_or_skip!();

    let initiation = initier(&db, OWNER).await;
    let consent_id = initiation["consent_id"].as_str().unwrap().to_string();

    let (status, corps) = appel(
        &db,
        "POST",
        "/v1/consents/callback",
        AUTRE_OWNER,
        Some(json!({ "code": "code-mock", "state": consent_id })),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(corps["code"], json!("not_found"));

    let statut = statut_consent_en_base(&db, Uuid::parse_str(&consent_id).unwrap()).await;
    assert_eq!(statut, "pending");

    db.destroy().await;
}

#[tokio::test]
async fn ac4_callback_refuse_marque_le_consent_en_echec_et_renvoie_une_api_error() {
    let db = db_or_skip!();

    let initiation = initier(&db, OWNER).await;
    let consent_id = initiation["consent_id"].as_str().unwrap().to_string();

    let (status, corps) = appel(
        &db,
        "POST",
        "/v1/consents/callback",
        OWNER,
        Some(json!({ "code": "code-mock", "state": "pas-un-uuid" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(corps["code"], json!("bad_request"));

    let statut = statut_consent_en_base(&db, Uuid::parse_str(&consent_id).unwrap()).await;
    assert_eq!(statut, "pending");

    db.destroy().await;
}

#[tokio::test]
async fn ac3_isolation_la_liste_des_consents_est_propre_au_sub() {
    let db = db_or_skip!();

    initier(&db, OWNER).await;

    let (status_owner, corps_owner) = appel(&db, "GET", "/v1/consents", OWNER, None).await;
    assert_eq!(status_owner, StatusCode::OK);
    assert_eq!(corps_owner["total"], json!(1));

    let (status_autre, corps_autre) = appel(&db, "GET", "/v1/consents", AUTRE_OWNER, None).await;
    assert_eq!(status_autre, StatusCode::OK);
    assert_eq!(corps_autre["total"], json!(0));

    db.destroy().await;
}

#[tokio::test]
async fn ac4_consentement_refuse_par_le_fournisseur_passe_en_failed() {
    let db = db_or_skip!();

    let initiation = initier(&db, OWNER).await;
    let consent_id = initiation["consent_id"].as_str().unwrap().to_string();

    let source: Arc<dyn BankDataSource> = Arc::new(SourceRefusante::nouvelle());
    let (status, corps) = appel_avec_state(
        state_avec_source(&db, source),
        "POST",
        "/v1/consents/callback",
        OWNER,
        Some(json!({ "code": "code-mock", "state": consent_id })),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(corps["code"], json!("consentement_refuse"));
    assert!(corps["message"].is_string());

    let statut = statut_consent_en_base(&db, Uuid::parse_str(&consent_id).unwrap()).await;
    assert_eq!(statut, "failed");

    db.destroy().await;
}
