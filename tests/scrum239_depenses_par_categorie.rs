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
use ch_api_budgy::repository::depenses::SqlxDepensesRepository;
use ch_api_budgy::repository::regles_categorisation::SqlxReglesCategorisationRepository;
use ch_api_budgy::routes::router;
use ch_api_budgy::services::jwt::JwtService;
use ch_api_budgy::state::AppState;
use chrono::{NaiveDate, TimeZone, Utc};
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
const ALICE: &str = "qvl-sub-239-alice";
const BOB: &str = "qvl-sub-239-bob";
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
                    "SCRUM-239 ignoré : variable {} absente (Postgres jetable requis)",
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

async fn semer_transaction(
    db: &DisposableDb,
    compte: &BankAccountId,
    amount_cents: i64,
    booking: NaiveDate,
) -> Uuid {
    let crypto = crypto();
    let inseree = BankTransactionsWriteRepository::enregistrer(
        &SqlxBankTransactionsWriteAdapter::new(db.pool.clone(), crypto),
        NouvelleTransactionBancaire {
            bank_account: compte.clone(),
            external_transaction_id: format!("tx-{}", Uuid::new_v4()),
            status: TransactionStatus::Booked,
            label: "OPERATION".to_string(),
            amount_cents,
            currency: "EUR".to_string(),
            booking_date: Some(booking),
            value_date: Some(booking),
        },
    )
    .await
    .expect("transaction semée");
    match inseree {
        ResultatInsertion::Inseree(id) => id.0,
        ResultatInsertion::Doublon => panic!("la transaction devait être insérée"),
    }
}

async fn categorie_depense_par_defaut(db: &DisposableDb, sub: &str) -> (String, String) {
    let (status, corps) = appel(db, "GET", "/v1/categories", sub, None).await;
    assert_eq!(status, StatusCode::OK);
    let categorie = corps["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["is_default"] == json!(true) && c["kind"] == json!("depense"))
        .expect("au moins une catégorie de dépense par défaut");
    (
        categorie["id"].as_str().unwrap().to_string(),
        categorie["name"].as_str().unwrap().to_string(),
    )
}

async fn creer_categorie_depense(db: &DisposableDb, sub: &str, nom: &str) -> String {
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

async fn categoriser(db: &DisposableDb, sub: &str, compte: &BankAccountId, transaction: Uuid, categorie: &str) {
    let (status, corps) = appel(
        db,
        "PUT",
        &format!(
            "/v1/accounts/{}/transactions/{transaction}/category",
            compte.0
        ),
        sub,
        Some(json!({ "category_id": categorie })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "catégorisation attendue OK : {corps}");
}

async fn depenses_du_mois(db: &DisposableDb, sub: &str, mois: &str) -> (StatusCode, Value) {
    appel(
        db,
        "GET",
        &format!("/v1/expenses/by-category?month={mois}"),
        sub,
        None,
    )
    .await
}

fn entree_par_categorie<'a>(corps: &'a Value, category_id: &str) -> &'a Value {
    corps["categories"]
        .as_array()
        .expect("categories est un tableau")
        .iter()
        .find(|e| e["category_id"] == json!(category_id))
        .unwrap_or_else(|| panic!("catégorie {category_id} absente de la répartition : {corps}"))
}

fn entree_non_categorisee(corps: &Value) -> &Value {
    corps["categories"]
        .as_array()
        .expect("categories est un tableau")
        .iter()
        .find(|e| e["category_id"].is_null())
        .unwrap_or_else(|| panic!("entrée non catégorisée absente : {corps}"))
}

fn jour(annee: i32, mois: u32, jour: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(annee, mois, jour).unwrap()
}

#[tokio::test]
async fn ca01_total_du_mois_et_repartition_par_categorie() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let (courses_id, courses_nom) = categorie_depense_par_defaut(&db, ALICE).await;
    let loisirs_id = creer_categorie_depense(&db, ALICE, "Loisirs 239").await;

    let t1 = semer_transaction(&db, &compte, -3_000, jour(2026, 6, 3)).await;
    let t2 = semer_transaction(&db, &compte, -1_500, jour(2026, 6, 12)).await;
    let t3 = semer_transaction(&db, &compte, -2_000, jour(2026, 6, 20)).await;
    categoriser(&db, ALICE, &compte, t1, &courses_id).await;
    categoriser(&db, ALICE, &compte, t2, &courses_id).await;
    categoriser(&db, ALICE, &compte, t3, &loisirs_id).await;

    let (status, corps) = depenses_du_mois(&db, ALICE, "2026-06").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(corps["month"], json!("2026-06"));
    assert_eq!(corps["total_cents"], json!(6_500));

    let courses = entree_par_categorie(&corps, &courses_id);
    assert_eq!(courses["amount_cents"], json!(4_500));
    assert_eq!(courses["category_name"], json!(courses_nom));
    assert_eq!(courses["kind"], json!("depense"));
    assert!(courses.get("color").is_some());
    assert!(courses.get("icon").is_some());

    let loisirs = entree_par_categorie(&corps, &loisirs_id);
    assert_eq!(loisirs["amount_cents"], json!(2_000));

    db.destroy().await;
}

#[tokio::test]
async fn ca01_montants_exposes_en_magnitude_positive() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let (categorie_id, _) = categorie_depense_par_defaut(&db, ALICE).await;
    let transaction = semer_transaction(&db, &compte, -4_590, jour(2026, 6, 8)).await;
    categoriser(&db, ALICE, &compte, transaction, &categorie_id).await;

    let (status, corps) = depenses_du_mois(&db, ALICE, "2026-06").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(corps["total_cents"], json!(4_590));
    assert!(corps["total_cents"].as_i64().unwrap() > 0);
    assert_eq!(entree_par_categorie(&corps, &categorie_id)["amount_cents"], json!(4_590));

    db.destroy().await;
}

#[tokio::test]
async fn ca01_les_credits_ne_comptent_pas_comme_depenses() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let (categorie_id, _) = categorie_depense_par_defaut(&db, ALICE).await;
    let depense = semer_transaction(&db, &compte, -1_000, jour(2026, 6, 5)).await;
    let credit = semer_transaction(&db, &compte, 5_000, jour(2026, 6, 6)).await;
    categoriser(&db, ALICE, &compte, depense, &categorie_id).await;
    categoriser(&db, ALICE, &compte, credit, &categorie_id).await;

    let (status, corps) = depenses_du_mois(&db, ALICE, "2026-06").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(corps["total_cents"], json!(1_000));
    let somme: i64 = corps["categories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["amount_cents"].as_i64().unwrap())
        .sum();
    assert_eq!(somme, 1_000);

    db.destroy().await;
}

#[tokio::test]
async fn depense_non_categorisee_expose_des_champs_categorie_nuls() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    semer_transaction(&db, &compte, -2_500, jour(2026, 6, 15)).await;

    let (status, corps) = depenses_du_mois(&db, ALICE, "2026-06").await;

    assert_eq!(status, StatusCode::OK);
    let entree = entree_non_categorisee(&corps);
    assert!(entree["category_id"].is_null());
    assert!(entree["category_name"].is_null());
    assert!(entree["kind"].is_null());
    assert!(entree["color"].is_null());
    assert!(entree["icon"].is_null());
    assert_eq!(entree["amount_cents"], json!(2_500));
    assert_eq!(corps["total_cents"], json!(2_500));

    db.destroy().await;
}

#[tokio::test]
async fn ca02_le_filtre_mois_isole_le_mois_demande() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let (categorie_id, _) = categorie_depense_par_defaut(&db, ALICE).await;
    let juin = semer_transaction(&db, &compte, -1_000, jour(2026, 6, 10)).await;
    let mai = semer_transaction(&db, &compte, -9_999, jour(2026, 5, 10)).await;
    categoriser(&db, ALICE, &compte, juin, &categorie_id).await;
    categoriser(&db, ALICE, &compte, mai, &categorie_id).await;

    let (status_juin, corps_juin) = depenses_du_mois(&db, ALICE, "2026-06").await;
    assert_eq!(status_juin, StatusCode::OK);
    assert_eq!(corps_juin["month"], json!("2026-06"));
    assert_eq!(corps_juin["total_cents"], json!(1_000));

    let (status_mai, corps_mai) = depenses_du_mois(&db, ALICE, "2026-05").await;
    assert_eq!(status_mai, StatusCode::OK);
    assert_eq!(corps_mai["month"], json!("2026-05"));
    assert_eq!(corps_mai["total_cents"], json!(9_999));

    db.destroy().await;
}

#[tokio::test]
async fn ca02_mois_sans_depense_renvoie_un_total_nul() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let (categorie_id, _) = categorie_depense_par_defaut(&db, ALICE).await;
    let transaction = semer_transaction(&db, &compte, -1_000, jour(2026, 6, 10)).await;
    categoriser(&db, ALICE, &compte, transaction, &categorie_id).await;

    let (status, corps) = depenses_du_mois(&db, ALICE, "2026-07").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(corps["month"], json!("2026-07"));
    assert_eq!(corps["total_cents"], json!(0));
    assert!(corps["categories"].as_array().unwrap().is_empty());

    db.destroy().await;
}

#[tokio::test]
async fn idor_les_depenses_d_un_autre_owner_ne_sont_pas_comptees() {
    let db = db_or_skip!();
    let compte_alice = semer_compte(&db, ALICE).await;
    let (categorie_alice, _) = categorie_depense_par_defaut(&db, ALICE).await;
    let depense_alice = semer_transaction(&db, &compte_alice, -8_000, jour(2026, 6, 4)).await;
    categoriser(&db, ALICE, &compte_alice, depense_alice, &categorie_alice).await;

    let (status_bob, corps_bob) = depenses_du_mois(&db, BOB, "2026-06").await;
    assert_eq!(status_bob, StatusCode::OK);
    assert_eq!(corps_bob["total_cents"], json!(0));
    assert!(corps_bob["categories"].as_array().unwrap().is_empty());

    let (status_alice, corps_alice) = depenses_du_mois(&db, ALICE, "2026-06").await;
    assert_eq!(status_alice, StatusCode::OK);
    assert_eq!(corps_alice["total_cents"], json!(8_000));

    db.destroy().await;
}
