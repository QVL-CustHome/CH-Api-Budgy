mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use ch_api_budgy::adapters::bank::selection::{SourceBancaire, construire_source};
use ch_api_budgy::config::EnableBankingConfig;
use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::repository::bank_accounts::SqlxBankAccountsWriteAdapter;
use ch_api_budgy::repository::bank_transactions::SqlxBankTransactionsWriteAdapter;
use ch_api_budgy::repository::budgets::SqlxBudgetsRepository;
use ch_api_budgy::repository::categories::SqlxCategoriesRepository;
use ch_api_budgy::repository::depenses::SqlxDepensesRepository;
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
use ch_api_budgy::repository::regles_categorisation::SqlxReglesCategorisationRepository;
use ch_api_budgy::routes::router;
use ch_api_budgy::services::jwt::JwtService;
use ch_api_budgy::state::AppState;
use common::DisposableDb;
use http_body_util::BodyExt;
use jsonwebtoken::{EncodingKey, Header};
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;

const SECRET: &str = "secret_de_test_budgy_32_octets_minimum_ok!!";
const ISSUER: &str = "ch-api-authenticator";
const AUDIENCE: &str = "ch-api-budgy";
const ALICE: &str = "qvl-sub-234-alice";
const BOB: &str = "qvl-sub-234-bob";
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
                    "SCRUM-234 ignoré : variable {} absente (Postgres jetable requis)",
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

async fn definir_budget(db: &DisposableDb, sub: &str, corps: Value) -> (StatusCode, Value) {
    appel(db, "POST", "/v1/budgets", sub, Some(corps)).await
}

async fn lister_budgets(db: &DisposableDb, sub: &str, mois: &str) -> (StatusCode, Value) {
    appel(db, "GET", &format!("/v1/budgets?mois={mois}"), sub, None).await
}

async fn categorie_de_depense(db: &DisposableDb, sub: &str) -> String {
    let (status, corps) = appel(db, "GET", "/v1/categories", sub, None).await;
    assert_eq!(status, StatusCode::OK);
    corps["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == json!("depense"))
        .expect("au moins une catégorie de dépense")["id"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn budgets_persistes(db: &DisposableDb, owner: &str) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM budgy.budgets WHERE owner_id = $1")
        .bind(owner)
        .fetch_one(&db.pool)
        .await
        .expect("comptage des budgets")
}

#[tokio::test]
async fn ca01_definir_un_budget_l_enregistre_pour_le_mois() {
    let db = db_or_skip!();
    let categorie = categorie_de_depense(&db, ALICE).await;

    let (status, corps) = definir_budget(
        &db,
        ALICE,
        json!({ "category_id": categorie, "montant_cents": 30_000, "mois": MOIS }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(corps["category_id"], json!(categorie));
    assert_eq!(corps["montant_cents"], json!(30_000));
    assert_eq!(corps["mois"], json!(MOIS));

    let (status_liste, liste) = lister_budgets(&db, ALICE, MOIS).await;
    assert_eq!(status_liste, StatusCode::OK);
    let budgets = liste["data"].as_array().unwrap();
    assert_eq!(budgets.len(), 1);
    assert_eq!(budgets[0]["category_id"], json!(categorie));
    assert_eq!(budgets[0]["montant_cents"], json!(30_000));
    assert_eq!(budgets[0]["mois"], json!(MOIS));

    db.destroy().await;
}

#[tokio::test]
async fn ca01_montant_cents_est_un_entier_conserve_a_l_identique() {
    let db = db_or_skip!();
    let categorie = categorie_de_depense(&db, ALICE).await;

    let (_, corps) = definir_budget(
        &db,
        ALICE,
        json!({ "category_id": categorie, "montant_cents": 4_250, "mois": MOIS }),
    )
    .await;
    assert_eq!(corps["montant_cents"], json!(4_250));
    assert!(corps["montant_cents"].is_i64());

    let (_, liste) = lister_budgets(&db, ALICE, MOIS).await;
    assert_eq!(liste["data"][0]["montant_cents"], json!(4_250));
    assert!(liste["data"][0]["montant_cents"].is_i64());

    db.destroy().await;
}

#[tokio::test]
async fn ca01_budget_est_rattache_au_mois_demande_uniquement() {
    let db = db_or_skip!();
    let categorie = categorie_de_depense(&db, ALICE).await;

    definir_budget(
        &db,
        ALICE,
        json!({ "category_id": categorie, "montant_cents": 12_000, "mois": "2026-07" }),
    )
    .await;

    let (_, juillet) = lister_budgets(&db, ALICE, "2026-07").await;
    let (_, aout) = lister_budgets(&db, ALICE, "2026-08").await;

    assert_eq!(juillet["data"].as_array().unwrap().len(), 1);
    assert_eq!(aout["data"].as_array().unwrap().len(), 0);

    db.destroy().await;
}

#[tokio::test]
async fn ca01_redefinir_le_budget_du_mois_met_a_jour_sans_doublon() {
    let db = db_or_skip!();
    let categorie = categorie_de_depense(&db, ALICE).await;

    definir_budget(
        &db,
        ALICE,
        json!({ "category_id": categorie, "montant_cents": 20_000, "mois": MOIS }),
    )
    .await;
    let (status, _) = definir_budget(
        &db,
        ALICE,
        json!({ "category_id": categorie, "montant_cents": 25_000, "mois": MOIS }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (_, liste) = lister_budgets(&db, ALICE, MOIS).await;
    let budgets = liste["data"].as_array().unwrap();
    assert_eq!(budgets.len(), 1);
    assert_eq!(budgets[0]["montant_cents"], json!(25_000));

    db.destroy().await;
}

#[tokio::test]
async fn ca02_montant_negatif_est_rejete_sans_enregistrement() {
    let db = db_or_skip!();
    let categorie = categorie_de_depense(&db, ALICE).await;

    let (status, _) = definir_budget(
        &db,
        ALICE,
        json!({ "category_id": categorie, "montant_cents": -100, "mois": MOIS }),
    )
    .await;

    assert!(status.is_client_error());
    assert_ne!(status, StatusCode::CREATED);
    assert_eq!(budgets_persistes(&db, ALICE).await, 0);

    let (_, liste) = lister_budgets(&db, ALICE, MOIS).await;
    assert_eq!(liste["data"].as_array().unwrap().len(), 0);

    db.destroy().await;
}

#[tokio::test]
async fn ca02_montant_non_numerique_est_rejete_sans_enregistrement() {
    let db = db_or_skip!();
    let categorie = categorie_de_depense(&db, ALICE).await;

    let (status, _) = definir_budget(
        &db,
        ALICE,
        json!({ "category_id": categorie, "montant_cents": "abc", "mois": MOIS }),
    )
    .await;

    assert!(status.is_client_error());
    assert_ne!(status, StatusCode::CREATED);
    assert_eq!(budgets_persistes(&db, ALICE).await, 0);

    db.destroy().await;
}

#[tokio::test]
async fn ca02_montant_zero_est_accepte_a_la_borne_du_contrat() {
    let db = db_or_skip!();
    let categorie = categorie_de_depense(&db, ALICE).await;

    let (status, corps) = definir_budget(
        &db,
        ALICE,
        json!({ "category_id": categorie, "montant_cents": 0, "mois": MOIS }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(corps["montant_cents"], json!(0));

    db.destroy().await;
}

#[tokio::test]
async fn idor_le_budget_d_un_owner_est_invisible_pour_un_autre() {
    let db = db_or_skip!();
    let categorie_alice = categorie_de_depense(&db, ALICE).await;

    definir_budget(
        &db,
        ALICE,
        json!({ "category_id": categorie_alice, "montant_cents": 40_000, "mois": MOIS }),
    )
    .await;

    let (status, liste_bob) = lister_budgets(&db, BOB, MOIS).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(liste_bob["data"].as_array().unwrap().len(), 0);
    assert_eq!(budgets_persistes(&db, BOB).await, 0);
    assert_eq!(budgets_persistes(&db, ALICE).await, 1);

    db.destroy().await;
}
