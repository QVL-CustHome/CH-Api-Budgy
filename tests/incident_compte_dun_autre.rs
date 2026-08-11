//! Incident du 2026-08-11 : la banque a renvoyé le compte d'un utilisateur à la
//! session d'autorisation d'un autre, et Budgy l'a importé sans broncher — 174
//! opérations, libellés et solde compris.
//!
//! En production restreinte, Enable Banking ne renvoie que les comptes
//! whitelistés dans son Control Panel. Si le compte d'un utilisateur en sort (ce
//! qui est arrivé), c'est celui du voisin qui revient à sa place. Budgy ne peut
//! pas empêcher la banque de se tromper ; il peut refuser de le croire.
//!
//! On simule ici une source qui renvoie toujours le même compte, quel que soit
//! le demandeur.

mod common;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use ch_api_budgy::adapters::bank::mock::MockBankDataSource;
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
use ch_api_budgy::repository::budgets::SqlxBudgetsRepository;
use ch_api_budgy::repository::categories::SqlxCategoriesRepository;
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
use ch_api_budgy::repository::depenses::SqlxDepensesRepository;
use ch_api_budgy::repository::enveloppes::SqlxEnveloppesRepository;
use ch_api_budgy::repository::preferences::SqlxPreferencesRepository;
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
const MARTIN: &str = "owner-incident-martin";
const CHRISTELLE: &str = "owner-incident-christelle";
const CALLBACK_URL: &str = "https://budgy.custhome.app/banque/callback";

/// Le compte unique que la banque renvoie à tout le monde : c'est le cœur de
/// l'incident.
const COMPTE_WHITELISTE: &str = "compte-whiteliste-unique";

struct SourceQuiRendToujoursLeMemeCompte {
    mock: MockBankDataSource,
}

#[async_trait]
impl BankDataSource for SourceQuiRendToujoursLeMemeCompte {
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
        proprietaire: &ProprietaireId,
        reponse: ReponseAutorisation,
    ) -> Result<Consent, BankDataSourceError> {
        self.mock
            .completer_consentement(proprietaire, reponse)
            .await
    }

    async fn lister_comptes(
        &self,
        consent: &Consent,
    ) -> Result<Vec<BankAccount>, BankDataSourceError> {
        let mut comptes = self.mock.lister_comptes(consent).await?;
        comptes.truncate(1);
        // Quel que soit le demandeur, la banque rend le compte whitelisté.
        for compte in &mut comptes {
            compte.external_account_id = COMPTE_WHITELISTE.to_string();
        }
        Ok(comptes)
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
    let crypto = Arc::new(CryptoService::from_key(&[7u8; 32]).unwrap());
    AppState {
        consents: Arc::new(SqlxConsentsWriteAdapter::new(
            db.pool.clone(),
            crypto.clone(),
        )),
        categories: Arc::new(SqlxCategoriesRepository::new(db.pool.clone())),
        budgets: Arc::new(SqlxBudgetsRepository::new(db.pool.clone())),
        depenses: Arc::new(SqlxDepensesRepository::new(db.pool.clone(), crypto.clone())),
        enveloppes: Arc::new(SqlxEnveloppesRepository::new(
            db.pool.clone(),
            crypto.clone(),
        )),
        preferences: Arc::new(SqlxPreferencesRepository::new(db.pool.clone())),
        regles_categorisation: Arc::new(SqlxReglesCategorisationRepository::new(db.pool.clone())),
        bank_accounts: Arc::new(SqlxBankAccountsWriteAdapter::new(
            db.pool.clone(),
            crypto.clone(),
        )),
        bank_transactions: Arc::new(SqlxBankTransactionsWriteAdapter::new(
            db.pool.clone(),
            crypto.clone(),
        )),
        bank_source: Arc::new(SourceQuiRendToujoursLeMemeCompte {
            mock: MockBankDataSource::new(),
        }),
        bank_callback_url: CALLBACK_URL.to_string(),
        db: db.pool.clone(),
        crypto,
        jwt: Arc::new(JwtService::from_secret(TEST_SECRET, ISSUER, AUDIENCE)),
    }
}

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
    let json = serde_json::from_str(&texte).unwrap_or(Value::Null);
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

async fn rattacher(db: &DisposableDb, owner: &str) -> (StatusCode, Value, String) {
    let banks = appel(db, "GET", "/v1/banks", owner, None).await;
    let bank_id = banks.1["data"][0]["id"].as_str().unwrap().to_string();
    let initiation = appel(
        db,
        "POST",
        "/v1/consents",
        owner,
        Some(json!({ "bank_id": bank_id })),
    )
    .await;
    let consent_id = initiation.1["consent_id"].as_str().unwrap().to_string();

    let (status, corps) = appel(
        db,
        "POST",
        "/v1/consents/callback",
        owner,
        Some(json!({ "code": "code-mock", "state": consent_id })),
    )
    .await;
    (status, corps, consent_id)
}

async fn comptes_de(db: &DisposableDb, owner: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM budgy.bank_account WHERE owner_id = $1")
        .bind(owner)
        .fetch_one(&db.pool)
        .await
        .unwrap()
}

/// Le premier à rattacher ce compte le garde : rien ne change pour lui.
#[tokio::test]
async fn le_premier_rattachement_reussit() {
    let db = db_or_skip!();

    let (status, _, _) = rattacher(&db, MARTIN).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(comptes_de(&db, MARTIN).await, 1);

    db.destroy().await;
}

/// Le cœur du correctif : un second utilisateur à qui la banque rend le même
/// compte est refusé, et rien n'est importé chez lui.
#[tokio::test]
async fn le_compte_dun_autre_utilisateur_est_refuse() {
    let db = db_or_skip!();

    let (status, _, _) = rattacher(&db, MARTIN).await;
    assert_eq!(status, StatusCode::OK);

    let (status, corps, consent_christelle) = rattacher(&db, CHRISTELLE).await;

    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "le rattachement doit être refusé : {corps}"
    );
    assert_eq!(corps["code"], json!("conflict"), "{corps}");
    assert_eq!(
        comptes_de(&db, CHRISTELLE).await,
        0,
        "aucun compte ne doit être créé pour le second utilisateur"
    );
    assert_eq!(
        comptes_de(&db, MARTIN).await,
        1,
        "le compte du premier utilisateur reste intact"
    );

    // Le consentement est marqué en échec : il ne doit pas rester « actif »
    // alors qu'aucun compte n'y est rattaché, sinon la synchro le reprendrait.
    let statut: String = sqlx::query_scalar("SELECT status FROM budgy.consent WHERE id = $1")
        .bind(Uuid::parse_str(&consent_christelle).unwrap())
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(statut, "failed");

    db.destroy().await;
}

/// Le même utilisateur qui re-rattache son propre compte (renouvellement de
/// consentement tous les 90 jours) ne doit surtout pas être bloqué.
#[tokio::test]
async fn le_renouvellement_par_le_meme_proprietaire_reste_autorise() {
    let db = db_or_skip!();

    let (premier, _, _) = rattacher(&db, MARTIN).await;
    assert_eq!(premier, StatusCode::OK);

    let (second, corps, _) = rattacher(&db, MARTIN).await;
    assert_eq!(
        second,
        StatusCode::OK,
        "re-rattacher son propre compte doit rester possible : {corps}"
    );

    db.destroy().await;
}
