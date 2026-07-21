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
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
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
const ALICE: &str = "qvl-sub-231-alice";
const BOB: &str = "qvl-sub-231-bob";
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
                    "SCRUM-231 ignoré : variable {} absente (Postgres jetable requis)",
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
            label: "ACHAT".to_string(),
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

async fn deux_categories_par_defaut(db: &DisposableDb, sub: &str) -> (String, String) {
    let (status, corps) = appel(db, "GET", "/v1/categories", sub, None).await;
    assert_eq!(status, StatusCode::OK);
    let defauts: Vec<String> = corps["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["is_default"] == json!(true))
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect();
    assert!(defauts.len() >= 2, "deux catégories par défaut requises");
    (defauts[0].clone(), defauts[1].clone())
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

fn uri_categoriser(compte: &BankAccountId, transaction: Uuid) -> String {
    format!(
        "/v1/accounts/{}/transactions/{transaction}/category",
        compte.0
    )
}

fn uri_non_categorisees(compte: &BankAccountId) -> String {
    format!("/v1/accounts/{}/transactions?uncategorized=true", compte.0)
}

fn uri_transactions(compte: &BankAccountId) -> String {
    format!("/v1/accounts/{}/transactions", compte.0)
}

async fn categoriser(
    db: &DisposableDb,
    sub: &str,
    compte: &BankAccountId,
    transaction: Uuid,
    categorie: &str,
) -> (StatusCode, Value) {
    appel(
        db,
        "PUT",
        &uri_categoriser(compte, transaction),
        sub,
        Some(json!({ "category_id": categorie })),
    )
    .await
}

fn ids_de(corps: &Value) -> Vec<String> {
    corps["data"]
        .as_array()
        .expect("data est un tableau")
        .iter()
        .map(|t| t["id"].as_str().expect("id de transaction").to_string())
        .collect()
}

#[tokio::test]
async fn ca01_attribution_associe_la_categorie_en_source_manuelle() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let transaction = semer_transaction(&db, &compte).await;
    let categorie = categorie_par_defaut(&db, ALICE).await;

    let (status, corps) = categoriser(&db, ALICE, &compte, transaction, &categorie).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(corps["category_id"], json!(categorie));
    assert_eq!(corps["categorization_source"], json!("manual"));

    db.destroy().await;
}

#[tokio::test]
async fn ca01_attribution_retire_la_transaction_des_non_categorisees() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let transaction = semer_transaction(&db, &compte).await;
    let categorie = categorie_par_defaut(&db, ALICE).await;

    let (_, avant) = appel(&db, "GET", &uri_non_categorisees(&compte), ALICE, None).await;
    assert!(
        ids_de(&avant).contains(&transaction.to_string()),
        "la transaction non catégorisée doit d'abord apparaître dans le filtre"
    );

    let (status, _) = categoriser(&db, ALICE, &compte, transaction, &categorie).await;
    assert_eq!(status, StatusCode::OK);

    let (_, apres) = appel(&db, "GET", &uri_non_categorisees(&compte), ALICE, None).await;
    assert!(
        !ids_de(&apres).contains(&transaction.to_string()),
        "la transaction catégorisée ne doit plus apparaître dans les non catégorisées"
    );

    db.destroy().await;
}

#[tokio::test]
async fn ca02_modification_remplace_l_ancienne_categorie() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let transaction = semer_transaction(&db, &compte).await;
    let (ancienne, nouvelle) = deux_categories_par_defaut(&db, ALICE).await;

    let (status_initial, corps_initial) =
        categoriser(&db, ALICE, &compte, transaction, &ancienne).await;
    assert_eq!(status_initial, StatusCode::OK);
    assert_eq!(corps_initial["category_id"], json!(ancienne));

    let (status, corps) = categoriser(&db, ALICE, &compte, transaction, &nouvelle).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(corps["category_id"], json!(nouvelle));
    assert_ne!(corps["category_id"], json!(ancienne));
    assert_eq!(corps["categorization_source"], json!("manual"));

    db.destroy().await;
}

#[tokio::test]
async fn ca03_filtre_ne_renvoie_que_les_transactions_sans_categorie() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let sans_categorie_a = semer_transaction(&db, &compte).await;
    let sans_categorie_b = semer_transaction(&db, &compte).await;
    let categorisee = semer_transaction(&db, &compte).await;
    let categorie = categorie_par_defaut(&db, ALICE).await;

    let (status_cat, _) = categoriser(&db, ALICE, &compte, categorisee, &categorie).await;
    assert_eq!(status_cat, StatusCode::OK);

    let (status, corps) = appel(&db, "GET", &uri_non_categorisees(&compte), ALICE, None).await;

    assert_eq!(status, StatusCode::OK);
    let ids = ids_de(&corps);
    assert_eq!(corps["total"], json!(2));
    assert!(ids.contains(&sans_categorie_a.to_string()));
    assert!(ids.contains(&sans_categorie_b.to_string()));
    assert!(!ids.contains(&categorisee.to_string()));

    db.destroy().await;
}

#[tokio::test]
async fn ca03_sans_filtre_toutes_les_transactions_sont_visibles() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let sans_categorie = semer_transaction(&db, &compte).await;
    let categorisee = semer_transaction(&db, &compte).await;
    let categorie = categorie_par_defaut(&db, ALICE).await;
    let (status_cat, _) = categoriser(&db, ALICE, &compte, categorisee, &categorie).await;
    assert_eq!(status_cat, StatusCode::OK);

    let (status, corps) = appel(&db, "GET", &uri_transactions(&compte), ALICE, None).await;

    assert_eq!(status, StatusCode::OK);
    let ids = ids_de(&corps);
    assert_eq!(corps["total"], json!(2));
    assert!(ids.contains(&sans_categorie.to_string()));
    assert!(ids.contains(&categorisee.to_string()));

    db.destroy().await;
}

#[tokio::test]
async fn idor_categoriser_la_transaction_d_un_autre_owner_renvoie_404() {
    let db = db_or_skip!();
    let compte_bob = semer_compte(&db, BOB).await;
    let transaction_bob = semer_transaction(&db, &compte_bob).await;
    let categorie = categorie_par_defaut(&db, ALICE).await;

    let (status, corps) = categoriser(&db, ALICE, &compte_bob, transaction_bob, &categorie).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(corps["code"], json!("not_found"));

    db.destroy().await;
}

#[tokio::test]
async fn idor_categoriser_avec_la_categorie_d_un_autre_owner_renvoie_404() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let transaction = semer_transaction(&db, &compte).await;
    let categorie_bob = creer_categorie(&db, BOB, "Privé Bob").await;

    let (status, corps) = categoriser(&db, ALICE, &compte, transaction, &categorie_bob).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(corps["code"], json!("not_found"));

    db.destroy().await;
}

#[tokio::test]
async fn categoriser_avec_une_categorie_par_defaut_est_autorise() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let transaction = semer_transaction(&db, &compte).await;
    let categorie = categorie_par_defaut(&db, ALICE).await;

    let (status, corps) = categoriser(&db, ALICE, &compte, transaction, &categorie).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(corps["category_id"], json!(categorie));

    db.destroy().await;
}
