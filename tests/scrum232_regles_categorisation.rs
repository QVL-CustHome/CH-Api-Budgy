mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use ch_api_budgy::adapters::bank::selection::{SourceBancaire, construire_source};
use ch_api_budgy::config::EnableBankingConfig;
use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::domain::bank_account::{BankAccountId, NouveauBankAccount};
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::{ConsentId, ConsentStatus, NouveauConsent};
use ch_api_budgy::domain::ports::ecriture::{
    BankAccountsWriteRepository, BankTransactionsWriteRepository, ConsentsWriteRepository,
    ResultatInsertion,
};
use ch_api_budgy::domain::transaction_bancaire::{NouvelleTransactionBancaire, TransactionStatus};
use ch_api_budgy::repository::bank_accounts::SqlxBankAccountsWriteAdapter;
use ch_api_budgy::repository::bank_transactions::SqlxBankTransactionsWriteAdapter;
use ch_api_budgy::repository::categories::SqlxCategoriesRepository;
use ch_api_budgy::repository::budgets::SqlxBudgetsRepository;
use ch_api_budgy::repository::depenses::SqlxDepensesRepository;
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
use ch_api_budgy::repository::regles_categorisation::SqlxReglesCategorisationRepository;
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

const SECRET: &str = "secret_de_test_budgy_32_octets_minimum_ok!!";
const ISSUER: &str = "ch-api-authenticator";
const AUDIENCE: &str = "ch-api-budgy";
const ALICE: &str = "qvl-sub-232-alice";
const BOB: &str = "qvl-sub-232-bob";
const CALLBACK_URL: &str = "https://budgy.custhome.app/banque/callback";

macro_rules! db_or_skip {
    () => {
        match DisposableDb::create().await {
            Some(db) => {
                db.migrate().await;
                db
            }
            None => {
                eprintln!(
                    "SCRUM-232 ignoré : variable {} absente (Postgres jetable requis)",
                    common::ENV_ADMIN_URL
                );
                return;
            }
        }
    };
}

fn epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn bearer(sub: &str) -> String {
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

fn state(db: &DisposableDb) -> AppState {
    let crypto = crypto();
    AppState {
        consents: Arc::new(SqlxConsentsWriteAdapter::new(
            db.pool.clone(),
            crypto.clone(),
        )),
        categories: Arc::new(SqlxCategoriesRepository::new(db.pool.clone())),
        budgets: Arc::new(SqlxBudgetsRepository::new(db.pool.clone())),
        depenses: Arc::new(SqlxDepensesRepository::new(db.pool.clone(), crypto.clone())),
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
        .header("Authorization", bearer(sub));
    let body = match corps {
        Some(valeur) => {
            builder = builder.header("Content-Type", "application/json");
            Body::from(valeur.to_string())
        }
        None => Body::empty(),
    };
    let reponse = router(state(db))
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

async fn creer_regle(db: &DisposableDb, sub: &str, corps: Value) -> (StatusCode, Value) {
    appel(db, "POST", "/v1/categorization-rules", sub, Some(corps)).await
}

async fn categorie_par_defaut(db: &DisposableDb, sub: &str) -> String {
    let (status, corps) = appel(db, "GET", "/v1/categories", sub, None).await;
    assert_eq!(status, StatusCode::OK);
    corps["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["is_default"] == json!(true))
        .expect("au moins une catégorie par défaut")["id"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn creer_categorie(db: &DisposableDb, sub: &str, nom: &str) -> String {
    let (status, corps) = appel(
        db,
        "POST",
        "/v1/categories",
        sub,
        Some(json!({ "name": nom, "kind": "depense" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    corps["id"].as_str().expect("id présent").to_string()
}

async fn regles_persistees(db: &DisposableDb, owner: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM budgy.regles_categorisation WHERE owner_id = $1",
    )
    .bind(owner)
    .fetch_one(&db.pool)
    .await
    .expect("comptage des règles")
}

async fn regle_par_id(db: &DisposableDb, id: &str) -> Option<(String, String, i32)> {
    let uuid = Uuid::parse_str(id).expect("id de règle valide");
    sqlx::query_as::<_, (String, Uuid, i32)>(
        "SELECT label_pattern, category_id, priority FROM budgy.regles_categorisation WHERE id = $1",
    )
    .bind(uuid)
    .fetch_optional(&db.pool)
    .await
    .expect("lecture de la règle")
    .map(|(pattern, category, priority)| (pattern, category.to_string(), priority))
}

async fn semer_compte(db: &DisposableDb, sub: &str) -> BankAccountId {
    let proprietaire = ProprietaireId(sub.to_string());
    let crypto = crypto();
    let consent = semer_consent(db, &crypto, &proprietaire).await;
    BankAccountsWriteRepository::enregistrer(
        &SqlxBankAccountsWriteAdapter::new(db.pool.clone(), crypto.clone()),
        NouveauBankAccount {
            proprietaire,
            consent,
            external_account_id: format!("acct-{}", Uuid::new_v4()),
            iban: "FR7630006000011234567890189".to_string(),
            currency: "EUR".to_string(),
            next_sync_at: None,
        },
    )
    .await
    .expect("compte semé")
}

async fn semer_consent(
    db: &DisposableDb,
    crypto: &Arc<CryptoService>,
    proprietaire: &ProprietaireId,
) -> ConsentId {
    ConsentsWriteRepository::enregistrer(
        &SqlxConsentsWriteAdapter::new(db.pool.clone(), crypto.clone()),
        NouveauConsent {
            proprietaire: proprietaire.clone(),
            external_ref: format!("ref-{}", Uuid::new_v4()),
            status: ConsentStatus::Active,
            expires_at: Some(Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap()),
        },
    )
    .await
    .expect("consent semé")
}

async fn semer_transaction(db: &DisposableDb, compte: &BankAccountId) -> Uuid {
    let crypto = crypto();
    let inseree = BankTransactionsWriteRepository::enregistrer(
        &SqlxBankTransactionsWriteAdapter::new(db.pool.clone(), crypto),
        NouvelleTransactionBancaire {
            bank_account: compte.clone(),
            external_transaction_id: format!("tx-{}", Uuid::new_v4()),
            status: TransactionStatus::Booked,
            label: "MONOPRIX PARIS".to_string(),
            amount_cents: -4_590,
            currency: "EUR".to_string(),
            booking_date: None,
            value_date: None,
        },
    )
    .await
    .expect("transaction semée");
    match inseree {
        ResultatInsertion::Inseree(id) => id.0,
        ResultatInsertion::Doublon => panic!("la transaction devait être insérée"),
    }
}

async fn categoriser_manuellement(
    db: &DisposableDb,
    sub: &str,
    compte: &BankAccountId,
    transaction: Uuid,
    categorie: &str,
) -> (StatusCode, Value) {
    appel(
        db,
        "PUT",
        &format!(
            "/v1/accounts/{}/transactions/{transaction}/category",
            compte.0
        ),
        sub,
        Some(json!({ "category_id": categorie })),
    )
    .await
}

#[tokio::test]
async fn ca02_acceptation_cree_une_regle_active_persistee() {
    let db = db_or_skip!();
    let categorie = categorie_par_defaut(&db, ALICE).await;

    let (status, corps) = creer_regle(
        &db,
        ALICE,
        json!({ "label_pattern": "MONOPRIX", "category_id": categorie }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert!(corps["id"].is_string());
    assert_eq!(corps["label_pattern"], json!("MONOPRIX"));
    assert_eq!(corps["category_id"], json!(categorie));
    assert_eq!(corps["priority"], json!(0));
    assert!(corps["created_at"].is_string());

    let id = corps["id"].as_str().unwrap();
    let persistee = regle_par_id(&db, id)
        .await
        .expect("règle persistée en base");
    assert_eq!(persistee.0, "MONOPRIX");
    assert_eq!(persistee.1, categorie);
    assert_eq!(persistee.2, 0);

    db.destroy().await;
}

#[tokio::test]
async fn ca02_priority_fournie_est_conservee() {
    let db = db_or_skip!();
    let categorie = categorie_par_defaut(&db, ALICE).await;

    let (status, corps) = creer_regle(
        &db,
        ALICE,
        json!({ "label_pattern": "SNCF", "category_id": categorie, "priority": 7 }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(corps["priority"], json!(7));

    let id = corps["id"].as_str().unwrap();
    assert_eq!(regle_par_id(&db, id).await.unwrap().2, 7);

    db.destroy().await;
}

#[tokio::test]
async fn ca02_categorie_par_defaut_est_autorisee() {
    let db = db_or_skip!();
    let categorie = categorie_par_defaut(&db, ALICE).await;

    let (status, corps) = creer_regle(
        &db,
        ALICE,
        json!({ "label_pattern": "EDF", "category_id": categorie }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(corps["category_id"], json!(categorie));

    db.destroy().await;
}

#[tokio::test]
async fn ca02_categorie_creee_par_l_owner_est_autorisee() {
    let db = db_or_skip!();
    let categorie = creer_categorie(&db, ALICE, "Abonnements").await;

    let (status, corps) = creer_regle(
        &db,
        ALICE,
        json!({ "label_pattern": "NETFLIX", "category_id": categorie }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(corps["category_id"], json!(categorie));

    db.destroy().await;
}

#[tokio::test]
async fn ca02_limite_pattern_de_140_caracteres_est_accepte() {
    let db = db_or_skip!();
    let categorie = categorie_par_defaut(&db, ALICE).await;
    let pattern = "A".repeat(140);

    let (status, corps) = creer_regle(
        &db,
        ALICE,
        json!({ "label_pattern": pattern, "category_id": categorie }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(corps["label_pattern"], json!(pattern));

    db.destroy().await;
}

#[tokio::test]
async fn validation_pattern_vide_renvoie_400_sans_creation() {
    let db = db_or_skip!();
    let categorie = categorie_par_defaut(&db, ALICE).await;

    let (status, corps) = creer_regle(
        &db,
        ALICE,
        json!({ "label_pattern": "", "category_id": categorie }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(corps["code"], json!("bad_request"));
    assert_eq!(regles_persistees(&db, ALICE).await, 0);

    db.destroy().await;
}

#[tokio::test]
async fn validation_pattern_de_141_caracteres_renvoie_400_sans_creation() {
    let db = db_or_skip!();
    let categorie = categorie_par_defaut(&db, ALICE).await;
    let pattern = "A".repeat(141);

    let (status, corps) = creer_regle(
        &db,
        ALICE,
        json!({ "label_pattern": pattern, "category_id": categorie }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(corps["code"], json!("bad_request"));
    assert_eq!(regles_persistees(&db, ALICE).await, 0);

    db.destroy().await;
}

#[tokio::test]
async fn idor_categorie_inexistante_renvoie_404_sans_creation() {
    let db = db_or_skip!();
    let inexistante = Uuid::new_v4().to_string();

    let (status, corps) = creer_regle(
        &db,
        ALICE,
        json!({ "label_pattern": "FANTOME", "category_id": inexistante }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(corps["code"], json!("not_found"));
    assert_eq!(regles_persistees(&db, ALICE).await, 0);

    db.destroy().await;
}

#[tokio::test]
async fn idor_categorie_d_un_autre_owner_renvoie_404_jamais_403() {
    let db = db_or_skip!();
    let categorie_bob = creer_categorie(&db, BOB, "Privé Bob").await;

    let (status, corps) = creer_regle(
        &db,
        ALICE,
        json!({ "label_pattern": "DETOURNEMENT", "category_id": categorie_bob }),
    )
    .await;

    assert_ne!(status, StatusCode::FORBIDDEN);
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(corps["code"], json!("not_found"));
    assert_eq!(regles_persistees(&db, ALICE).await, 0);

    db.destroy().await;
}

#[tokio::test]
async fn ca03_categorisation_manuelle_seule_ne_cree_aucune_regle() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let transaction = semer_transaction(&db, &compte).await;
    let categorie = categorie_par_defaut(&db, ALICE).await;

    let (status, corps) =
        categoriser_manuellement(&db, ALICE, &compte, transaction, &categorie).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(corps["category_id"], json!(categorie));
    assert_eq!(corps["categorization_source"], json!("manual"));
    assert_eq!(regles_persistees(&db, ALICE).await, 0);

    db.destroy().await;
}
