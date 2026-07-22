mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use ch_api_budgy::adapters::bank::selection::{SourceBancaire, construire_source};
use ch_api_budgy::config::EnableBankingConfig;
use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::domain::balance::{BalanceType, NouvelleBalance};
use ch_api_budgy::domain::bank_account::{BankAccountId, NouveauBankAccount};
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::{ConsentId, ConsentStatus, NouveauConsent};
use ch_api_budgy::domain::ports::ecriture::{
    BalancesWriteRepository, BankAccountsWriteRepository, ConsentsWriteRepository,
};
use ch_api_budgy::repository::balances::SqlxBalancesWriteAdapter;
use ch_api_budgy::repository::bank_accounts::SqlxBankAccountsWriteAdapter;
use ch_api_budgy::repository::categories::SqlxCategoriesRepository;
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
use ch_api_budgy::repository::regles_categorisation::SqlxReglesCategorisationRepository;
use ch_api_budgy::repository::bank_transactions::SqlxBankTransactionsWriteAdapter;
use ch_api_budgy::routes::router;
use ch_api_budgy::services::jwt::JwtService;
use ch_api_budgy::state::AppState;
use chrono::{TimeZone, Utc};
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
const OWNER: &str = "owner-scrum-238";
const AUTRE_OWNER: &str = "owner-scrum-238-intrus";

fn crypto() -> Arc<CryptoService> {
    Arc::new(CryptoService::from_key(&[7u8; 32]).expect("clé de test 32 octets valide"))
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

fn state(db: &DisposableDb, crypto: &Arc<CryptoService>) -> AppState {
    AppState {
        consents: Arc::new(SqlxConsentsWriteAdapter::new(
            db.pool.clone(),
            crypto.clone(),
        )),
        categories: Arc::new(SqlxCategoriesRepository::new(db.pool.clone())),
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
        bank_callback_url: "https://budgy.custhome.app/banque/callback".to_string(),
        db: db.pool.clone(),
        crypto: crypto.clone(),
        jwt: Arc::new(JwtService::from_secret(TEST_SECRET, ISSUER, AUDIENCE)),
    }
}

async fn get_balance(db: &DisposableDb, crypto: &Arc<CryptoService>, owner: &str) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("GET")
        .uri("/v1/balance")
        .header("Authorization", bearer(owner))
        .body(Body::empty())
        .unwrap();
    let response = router(state(db, crypto)).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let corps = String::from_utf8(bytes.to_vec()).unwrap();
    let body = serde_json::from_str(&corps)
        .unwrap_or_else(|_| panic!("le corps n'est pas un JSON valide : {corps}"));
    (status, body)
}

async fn consent(db: &DisposableDb, crypto: &Arc<CryptoService>, owner: &str) -> ConsentId {
    ConsentsWriteRepository::enregistrer(
        &SqlxConsentsWriteAdapter::new(db.pool.clone(), crypto.clone()),
        NouveauConsent {
            proprietaire: ProprietaireId(owner.to_string()),
            external_ref: format!("ref-{owner}-{}", Uuid::new_v4()),
            status: ConsentStatus::Active,
            expires_at: Some(Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap()),
        },
    )
    .await
    .expect("consent enregistré")
}

async fn compte(
    db: &DisposableDb,
    crypto: &Arc<CryptoService>,
    owner: &str,
    consent: ConsentId,
    iban: &str,
) -> BankAccountId {
    BankAccountsWriteRepository::enregistrer(
        &SqlxBankAccountsWriteAdapter::new(db.pool.clone(), crypto.clone()),
        NouveauBankAccount {
            proprietaire: ProprietaireId(owner.to_string()),
            consent,
            external_account_id: format!("acct-{}", Uuid::new_v4()),
            iban: iban.to_string(),
            currency: "EUR".to_string(),
            next_sync_at: None,
        },
    )
    .await
    .expect("compte enregistré")
}

async fn balance(
    db: &DisposableDb,
    crypto: &Arc<CryptoService>,
    compte: &BankAccountId,
    balance_type: BalanceType,
    amount_cents: i64,
) {
    BalancesWriteRepository::enregistrer(
        &SqlxBalancesWriteAdapter::new(db.pool.clone(), crypto.clone()),
        NouvelleBalance {
            bank_account: compte.clone(),
            balance_type,
            amount_cents,
            currency: "EUR".to_string(),
            reference_date: Utc.with_ymd_and_hms(2026, 6, 20, 12, 0, 0).unwrap(),
        },
    )
    .await
    .expect("balance enregistrée");
}

fn solde_scalaire_entier(account: &Value) -> i64 {
    let balance = &account["balance"];
    assert!(
        !balance.is_object(),
        "balance ne doit jamais être un objet, trouvé : {balance}"
    );
    assert!(
        !balance.is_null(),
        "balance ne doit jamais être null, un compte sans solde vaut 0"
    );
    assert!(
        balance.is_i64(),
        "balance doit être un entier scalaire plat, trouvé : {balance}"
    );
    balance.as_i64().unwrap()
}

#[derive(serde::Deserialize)]
struct SoldeCompteStrict {
    id: String,
    iban_masked: String,
    currency: String,
    balance: i64,
}

#[derive(serde::Deserialize)]
struct SoldesConsolidesStrict {
    total_cents: i64,
    accounts: Vec<SoldeCompteStrict>,
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

#[tokio::test]
async fn ca01_solde_consolide_egale_somme_et_liste_chaque_compte() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = consent(&db, &crypto, OWNER).await;
    let compte_a = compte(&db, &crypto, OWNER, consent.clone(), "FR7630006000011234567890189").await;
    let compte_b = compte(&db, &crypto, OWNER, consent, "FR7610107001011234567890129").await;
    balance(&db, &crypto, &compte_a, BalanceType::Booked, 15_327).await;
    balance(&db, &crypto, &compte_b, BalanceType::Booked, 4_673).await;

    let (status, body) = get_balance(&db, &crypto, OWNER).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_cents"], json!(20_000));
    assert!(body["total_cents"].is_i64());

    let accounts = body["accounts"].as_array().expect("accounts est une liste");
    assert_eq!(accounts.len(), 2);

    let mut par_id: std::collections::HashMap<String, &Value> = std::collections::HashMap::new();
    for account in accounts {
        let id = account["id"].as_str().expect("id présent").to_string();
        assert!(account["iban_masked"].is_string());
        assert_eq!(account["currency"], json!("EUR"));
        assert!(account.get("iban").is_none());
        par_id.insert(id, account);
    }

    let a = par_id[&compte_a.0.to_string()];
    let b = par_id[&compte_b.0.to_string()];
    assert_eq!(solde_scalaire_entier(a), 15_327);
    assert_eq!(solde_scalaire_entier(b), 4_673);
    assert!(a["iban_masked"].as_str().unwrap().ends_with("0189"));
    assert!(!a["iban_masked"].as_str().unwrap().contains("FR76"));

    db.destroy().await;
}

#[tokio::test]
async fn ca02_aucun_compte_renvoie_total_zero_et_liste_vide() {
    let db = db_or_skip!();
    let crypto = crypto();

    let (status, body) = get_balance(&db, &crypto, OWNER).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_cents"], json!(0));
    assert!(body["total_cents"].is_i64());
    assert_eq!(body["accounts"], json!([]));

    db.destroy().await;
}

#[tokio::test]
async fn anti_idor_un_owner_ne_voit_que_ses_propres_comptes() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent_owner = consent(&db, &crypto, OWNER).await;
    let compte_owner = compte(&db, &crypto, OWNER, consent_owner, "FR7630006000011234567890189").await;
    balance(&db, &crypto, &compte_owner, BalanceType::Booked, 5_000).await;

    let consent_intrus = consent(&db, &crypto, AUTRE_OWNER).await;
    let compte_intrus = compte(&db, &crypto, AUTRE_OWNER, consent_intrus, "FR7610107001011234567890129").await;
    balance(&db, &crypto, &compte_intrus, BalanceType::Booked, 999_999).await;

    let (status, body) = get_balance(&db, &crypto, OWNER).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_cents"], json!(5_000));
    let accounts = body["accounts"].as_array().expect("accounts est une liste");
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0]["id"], json!(compte_owner.0.to_string()));

    db.destroy().await;
}

#[tokio::test]
async fn ca01_solde_zero_est_une_valeur_valide_dans_le_total() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = consent(&db, &crypto, OWNER).await;
    let compte_a = compte(&db, &crypto, OWNER, consent, "FR7630006000011234567890189").await;
    balance(&db, &crypto, &compte_a, BalanceType::Booked, 0).await;

    let (status, body) = get_balance(&db, &crypto, OWNER).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_cents"], json!(0));
    let accounts = body["accounts"].as_array().expect("accounts est une liste");
    assert_eq!(accounts.len(), 1);
    assert_eq!(solde_scalaire_entier(&accounts[0]), 0);

    db.destroy().await;
}

#[tokio::test]
async fn ca01_chaque_balance_deserialise_en_entier_scalaire_et_total_egale_la_somme() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = consent(&db, &crypto, OWNER).await;
    let compte_avec_solde =
        compte(&db, &crypto, OWNER, consent.clone(), "FR7630006000011234567890189").await;
    let compte_sans_solde =
        compte(&db, &crypto, OWNER, consent, "FR7610107001011234567890129").await;
    balance(&db, &crypto, &compte_avec_solde, BalanceType::Booked, 15_327).await;

    let (status, body) = get_balance(&db, &crypto, OWNER).await;

    assert_eq!(status, StatusCode::OK);
    let consolide: SoldesConsolidesStrict = serde_json::from_value(body).expect(
        "chaque accounts[].balance doit désérialiser en entier scalaire i64, jamais un objet ni null",
    );

    for compte in &consolide.accounts {
        assert_eq!(compte.currency, "EUR");
        assert!(!compte.iban_masked.is_empty());
    }

    let somme: i64 = consolide.accounts.iter().map(|compte| compte.balance).sum();
    assert_eq!(consolide.total_cents, somme);

    let par_id: std::collections::HashMap<&str, i64> = consolide
        .accounts
        .iter()
        .map(|compte| (compte.id.as_str(), compte.balance))
        .collect();
    assert_eq!(par_id[compte_avec_solde.0.to_string().as_str()], 15_327);
    assert_eq!(par_id[compte_sans_solde.0.to_string().as_str()], 0);
    assert_eq!(consolide.total_cents, 15_327);

    db.destroy().await;
}
