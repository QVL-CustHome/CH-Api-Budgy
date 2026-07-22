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
use ch_api_budgy::repository::budgets::SqlxBudgetsRepository;
use ch_api_budgy::repository::categories::SqlxCategoriesRepository;
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
use ch_api_budgy::repository::depenses::SqlxDepensesRepository;
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

const SECRET: &str = "secret_de_test_budgy_32_octets_minimum_ok!!";
const ISSUER: &str = "ch-api-authenticator";
const AUDIENCE: &str = "ch-api-budgy";
const ALICE: &str = "qvl-sub-235-alice";
const BOB: &str = "qvl-sub-235-bob";
const CALLBACK_URL: &str = "https://budgy.custhome.app/banque/callback";
const MOIS: &str = "2026-07";

macro_rules! db_or_skip {
    () => {
        match DisposableDb::create().await {
            Some(db) => {
                db.migrate().await;
                db
            }
            None => {
                eprintln!(
                    "SCRUM-235 ignoré : variable {} absente (Postgres jetable requis)",
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
            expires_at: None,
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

async fn categoriser(
    db: &DisposableDb,
    sub: &str,
    compte: &BankAccountId,
    transaction: Uuid,
    categorie: &str,
) {
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
    assert_eq!(
        status,
        StatusCode::OK,
        "catégorisation attendue OK : {corps}"
    );
}

async fn definir_budget(
    db: &DisposableDb,
    sub: &str,
    categorie: &str,
    montant_cents: i64,
    mois: &str,
) {
    let (status, corps) = appel(
        db,
        "POST",
        "/v1/budgets",
        sub,
        Some(json!({ "category_id": categorie, "montant_cents": montant_cents, "mois": mois })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "définition de budget attendue CREATED : {corps}"
    );
}

async fn reste_a_depenser(db: &DisposableDb, sub: &str, mois: &str) -> (StatusCode, Value) {
    appel(
        db,
        "GET",
        &format!("/v1/budgets/remaining?month={mois}"),
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
        .unwrap_or_else(|| panic!("catégorie {category_id} absente du reste à dépenser : {corps}"))
}

fn jour(annee: i32, mois: u32, jour: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(annee, mois, jour).unwrap()
}

#[tokio::test]
async fn ca01_reste_egal_budget_moins_somme_des_depenses_categorisees() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let (categorie_id, categorie_nom) = categorie_depense_par_defaut(&db, ALICE).await;
    definir_budget(&db, ALICE, &categorie_id, 30_000, MOIS).await;

    let t1 = semer_transaction(&db, &compte, -3_000, jour(2026, 7, 3)).await;
    let t2 = semer_transaction(&db, &compte, -1_500, jour(2026, 7, 12)).await;
    categoriser(&db, ALICE, &compte, t1, &categorie_id).await;
    categoriser(&db, ALICE, &compte, t2, &categorie_id).await;

    let (status, corps) = reste_a_depenser(&db, ALICE, MOIS).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(corps["month"], json!(MOIS));

    let entree = entree_par_categorie(&corps, &categorie_id);
    assert_eq!(entree["category_name"], json!(categorie_nom));
    assert_eq!(entree["kind"], json!("depense"));
    assert!(entree.get("color").is_some());
    assert!(entree.get("icon").is_some());
    assert_eq!(entree["montant_prevu_cents"], json!(30_000));
    assert_eq!(entree["depense_cents"], json!(4_500));
    assert_eq!(entree["reste_cents"], json!(25_500));
    assert_eq!(entree["depassement_cents"], json!(0));
    assert_eq!(entree["depasse"], json!(false));

    assert!(entree["reste_cents"].is_i64());
    assert!(entree["depense_cents"].is_i64());
    assert!(entree["montant_prevu_cents"].is_i64());

    db.destroy().await;
}

#[tokio::test]
async fn ca01_categorie_budgetee_sans_depense_le_reste_vaut_le_budget() {
    let db = db_or_skip!();
    let (categorie_id, _) = categorie_depense_par_defaut(&db, ALICE).await;
    definir_budget(&db, ALICE, &categorie_id, 18_000, MOIS).await;

    let (status, corps) = reste_a_depenser(&db, ALICE, MOIS).await;

    assert_eq!(status, StatusCode::OK);
    let entree = entree_par_categorie(&corps, &categorie_id);
    assert_eq!(entree["montant_prevu_cents"], json!(18_000));
    assert_eq!(entree["depense_cents"], json!(0));
    assert_eq!(entree["reste_cents"], json!(18_000));
    assert_eq!(entree["depassement_cents"], json!(0));
    assert_eq!(entree["depasse"], json!(false));

    db.destroy().await;
}

#[tokio::test]
async fn ca01_depenses_egales_au_budget_donnent_un_reste_nul_sans_depassement() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let (categorie_id, _) = categorie_depense_par_defaut(&db, ALICE).await;
    definir_budget(&db, ALICE, &categorie_id, 20_000, MOIS).await;

    let t1 = semer_transaction(&db, &compte, -20_000, jour(2026, 7, 9)).await;
    categoriser(&db, ALICE, &compte, t1, &categorie_id).await;

    let (status, corps) = reste_a_depenser(&db, ALICE, MOIS).await;

    assert_eq!(status, StatusCode::OK);
    let entree = entree_par_categorie(&corps, &categorie_id);
    assert_eq!(entree["montant_prevu_cents"], json!(20_000));
    assert_eq!(entree["depense_cents"], json!(20_000));
    assert_eq!(entree["reste_cents"], json!(0));
    assert_eq!(entree["depassement_cents"], json!(0));
    assert_eq!(entree["depasse"], json!(false));

    db.destroy().await;
}

#[tokio::test]
async fn ca02_depassement_expose_reste_negatif_et_montant_depasse_exact() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let (categorie_id, _) = categorie_depense_par_defaut(&db, ALICE).await;
    definir_budget(&db, ALICE, &categorie_id, 10_000, MOIS).await;

    let t1 = semer_transaction(&db, &compte, -9_000, jour(2026, 7, 4)).await;
    let t2 = semer_transaction(&db, &compte, -6_000, jour(2026, 7, 18)).await;
    categoriser(&db, ALICE, &compte, t1, &categorie_id).await;
    categoriser(&db, ALICE, &compte, t2, &categorie_id).await;

    let (status, corps) = reste_a_depenser(&db, ALICE, MOIS).await;

    assert_eq!(status, StatusCode::OK);
    let entree = entree_par_categorie(&corps, &categorie_id);
    assert_eq!(entree["montant_prevu_cents"], json!(10_000));
    assert_eq!(entree["depense_cents"], json!(15_000));
    assert_eq!(entree["reste_cents"], json!(-5_000));
    assert_eq!(entree["depassement_cents"], json!(5_000));
    assert_eq!(entree["depasse"], json!(true));

    db.destroy().await;
}

#[tokio::test]
async fn ca01_les_depenses_d_un_autre_mois_ne_sont_pas_comptees() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let (categorie_id, _) = categorie_depense_par_defaut(&db, ALICE).await;
    definir_budget(&db, ALICE, &categorie_id, 30_000, MOIS).await;

    let juillet = semer_transaction(&db, &compte, -4_000, jour(2026, 7, 10)).await;
    let juin = semer_transaction(&db, &compte, -25_000, jour(2026, 6, 10)).await;
    let aout = semer_transaction(&db, &compte, -9_999, jour(2026, 8, 10)).await;
    categoriser(&db, ALICE, &compte, juillet, &categorie_id).await;
    categoriser(&db, ALICE, &compte, juin, &categorie_id).await;
    categoriser(&db, ALICE, &compte, aout, &categorie_id).await;

    let (status, corps) = reste_a_depenser(&db, ALICE, MOIS).await;

    assert_eq!(status, StatusCode::OK);
    let entree = entree_par_categorie(&corps, &categorie_id);
    assert_eq!(entree["depense_cents"], json!(4_000));
    assert_eq!(entree["reste_cents"], json!(26_000));
    assert_eq!(entree["depasse"], json!(false));

    db.destroy().await;
}

#[tokio::test]
async fn idor_le_reste_d_un_owner_ignore_les_budgets_et_depenses_d_un_autre() {
    let db = db_or_skip!();
    let compte_alice = semer_compte(&db, ALICE).await;
    let (categorie_alice, _) = categorie_depense_par_defaut(&db, ALICE).await;
    definir_budget(&db, ALICE, &categorie_alice, 30_000, MOIS).await;
    let depense_alice = semer_transaction(&db, &compte_alice, -5_000, jour(2026, 7, 6)).await;
    categoriser(&db, ALICE, &compte_alice, depense_alice, &categorie_alice).await;

    let compte_bob = semer_compte(&db, BOB).await;
    let (categorie_bob, _) = categorie_depense_par_defaut(&db, BOB).await;
    definir_budget(&db, BOB, &categorie_bob, 99_000, MOIS).await;
    let depense_bob = semer_transaction(&db, &compte_bob, -80_000, jour(2026, 7, 7)).await;
    categoriser(&db, BOB, &compte_bob, depense_bob, &categorie_bob).await;

    let (status_alice, corps_alice) = reste_a_depenser(&db, ALICE, MOIS).await;
    assert_eq!(status_alice, StatusCode::OK);
    let entree_alice = entree_par_categorie(&corps_alice, &categorie_alice);
    assert_eq!(entree_alice["montant_prevu_cents"], json!(30_000));
    assert_eq!(entree_alice["depense_cents"], json!(5_000));
    assert_eq!(entree_alice["reste_cents"], json!(25_000));

    let (status_bob, corps_bob) = reste_a_depenser(&db, BOB, MOIS).await;
    assert_eq!(status_bob, StatusCode::OK);
    let entree_bob = entree_par_categorie(&corps_bob, &categorie_bob);
    assert_eq!(entree_bob["montant_prevu_cents"], json!(99_000));
    assert_eq!(entree_bob["depense_cents"], json!(80_000));
    assert_eq!(entree_bob["reste_cents"], json!(19_000));

    db.destroy().await;
}

#[tokio::test]
async fn validation_month_absent_renvoie_400_bad_request() {
    let db = db_or_skip!();

    let (status, corps) = appel(&db, "GET", "/v1/budgets/remaining", ALICE, None).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(corps["code"], json!("bad_request"));

    db.destroy().await;
}

#[tokio::test]
async fn validation_month_mal_forme_renvoie_400_bad_request() {
    let db = db_or_skip!();

    let (status, corps) = reste_a_depenser(&db, ALICE, "2026-13").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(corps["code"], json!("bad_request"));

    let (status_texte, corps_texte) = reste_a_depenser(&db, ALICE, "juillet").await;
    assert_eq!(status_texte, StatusCode::BAD_REQUEST);
    assert_eq!(corps_texte["code"], json!("bad_request"));

    db.destroy().await;
}
