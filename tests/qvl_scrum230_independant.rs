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
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
use ch_api_budgy::repository::depenses::SqlxDepensesRepository;
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
const ALICE: &str = "qvl-sub-230-alice";
const BOB: &str = "qvl-sub-230-bob";
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

fn requete_categorie(nom: &str, kind: &str) -> Value {
    json!({ "name": nom, "kind": kind })
}

async fn lister(db: &DisposableDb, sub: &str) -> Value {
    let (status, corps) = appel(db, "GET", "/v1/categories", sub, None).await;
    assert_eq!(status, StatusCode::OK);
    corps
}

async fn creer(db: &DisposableDb, sub: &str, nom: &str, kind: &str) -> String {
    let (status, corps) = appel(
        db,
        "POST",
        "/v1/categories",
        sub,
        Some(requete_categorie(nom, kind)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    corps["id"].as_str().expect("id présent").to_string()
}

fn total(corps: &Value) -> u64 {
    corps["data"].as_array().expect("data est un tableau").len() as u64
}

fn contient_nom(corps: &Value, nom: &str) -> bool {
    corps["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["name"] == json!(nom))
}

fn id_categorie_par_defaut(corps: &Value) -> String {
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

#[tokio::test]
async fn ac1_liste_expose_les_categories_par_defaut_pour_un_nouvel_utilisateur() {
    let db = db_ou_skip!();

    let corps = lister(&db, ALICE).await;

    assert!(contient_nom(&corps, "Salaire"));
    assert!(contient_nom(&corps, "Loyer"));
    assert!(total(&corps) >= 10);

    db.destroy().await;
}

#[tokio::test]
async fn ac1_liste_inclut_les_categories_creees_par_l_utilisateur() {
    let db = db_ou_skip!();

    creer(&db, ALICE, "Abonnements", "depense").await;
    let corps = lister(&db, ALICE).await;

    assert!(contient_nom(&corps, "Abonnements"));
    let creee = corps["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == json!("Abonnements"))
        .unwrap();
    assert_eq!(creee["is_default"], json!(false));

    db.destroy().await;
}

#[tokio::test]
async fn ac1_enveloppe_contient_data_et_total_coherents() {
    let db = db_ou_skip!();

    creer(&db, ALICE, "Épargne", "revenu").await;
    let corps = lister(&db, ALICE).await;

    assert!(corps["data"].is_array());
    assert!(corps["total"].is_number());
    assert_eq!(corps["total"], json!(total(&corps)));

    db.destroy().await;
}

#[tokio::test]
async fn ac1_liste_est_scopee_par_utilisateur() {
    let db = db_ou_skip!();

    creer(&db, ALICE, "Perso Alice", "depense").await;

    let vue_bob = lister(&db, BOB).await;

    assert!(!contient_nom(&vue_bob, "Perso Alice"));
    assert!(contient_nom(&vue_bob, "Salaire"));

    db.destroy().await;
}

#[tokio::test]
async fn ac2_creation_renvoie_201_categorie_non_par_defaut() {
    let db = db_ou_skip!();

    let (status, corps) = appel(
        &db,
        "POST",
        "/v1/categories",
        ALICE,
        Some(requete_categorie("Vacances", "depense")),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert!(corps["id"].is_string());
    assert_eq!(corps["name"], json!("Vacances"));
    assert_eq!(corps["is_default"], json!(false));

    db.destroy().await;
}

#[tokio::test]
async fn ac2_nom_d_un_caractere_est_accepte() {
    let db = db_ou_skip!();

    let (status, _) = appel(
        &db,
        "POST",
        "/v1/categories",
        ALICE,
        Some(requete_categorie("A", "depense")),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);

    db.destroy().await;
}

#[tokio::test]
async fn ac2_nom_de_trente_caracteres_est_accepte() {
    let db = db_ou_skip!();
    let nom = "A".repeat(30);

    let (status, corps) = appel(
        &db,
        "POST",
        "/v1/categories",
        ALICE,
        Some(requete_categorie(&nom, "depense")),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(corps["name"], json!(nom));

    db.destroy().await;
}

#[tokio::test]
async fn ac2_nom_vide_est_rejete_sans_creation() {
    let db = db_ou_skip!();
    let avant = total(&lister(&db, ALICE).await);

    let (status, corps) = appel(
        &db,
        "POST",
        "/v1/categories",
        ALICE,
        Some(requete_categorie("", "depense")),
    )
    .await;

    assert!(status.is_client_error());
    assert_ne!(status, StatusCode::CREATED);
    assert!(corps["code"].is_string());
    assert!(corps["message"].is_string());

    let apres = total(&lister(&db, ALICE).await);
    assert_eq!(avant, apres);

    db.destroy().await;
}

#[tokio::test]
async fn ac2_nom_de_trente_et_un_caracteres_est_rejete_sans_creation() {
    let db = db_ou_skip!();
    let nom = "A".repeat(31);
    let avant = total(&lister(&db, ALICE).await);

    let (status, corps) = appel(
        &db,
        "POST",
        "/v1/categories",
        ALICE,
        Some(requete_categorie(&nom, "depense")),
    )
    .await;

    assert!(status.is_client_error());
    assert_ne!(status, StatusCode::CREATED);
    assert!(corps["code"].is_string());
    assert!(corps["message"].is_string());

    let apres = total(&lister(&db, ALICE).await);
    assert_eq!(avant, apres);

    db.destroy().await;
}

#[tokio::test]
async fn ac3_modification_de_sa_categorie_renvoie_200_et_persiste() {
    let db = db_ou_skip!();
    let id = creer(&db, ALICE, "Sport", "depense").await;

    let (status, corps) = appel(
        &db,
        "PUT",
        &format!("/v1/categories/{id}"),
        ALICE,
        Some(requete_categorie("Sport et loisirs", "depense")),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(corps["name"], json!("Sport et loisirs"));

    let liste = lister(&db, ALICE).await;
    assert!(contient_nom(&liste, "Sport et loisirs"));
    assert!(!contient_nom(&liste, "Sport"));

    db.destroy().await;
}

#[tokio::test]
async fn ac4_suppression_de_sa_categorie_renvoie_204_et_disparait() {
    let db = db_ou_skip!();
    let id = creer(&db, ALICE, "Temporaire", "depense").await;

    let (status, corps) = appel(&db, "DELETE", &format!("/v1/categories/{id}"), ALICE, None).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(corps, Value::Null);

    let liste = lister(&db, ALICE).await;
    assert!(!contient_nom(&liste, "Temporaire"));

    db.destroy().await;
}

#[tokio::test]
async fn ac5_modification_categorie_d_un_autre_utilisateur_renvoie_404() {
    let db = db_ou_skip!();
    let id = creer(&db, ALICE, "Privé Alice", "depense").await;

    let (status, corps) = appel(
        &db,
        "PUT",
        &format!("/v1/categories/{id}"),
        BOB,
        Some(requete_categorie("Détourné par Bob", "depense")),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(corps["code"], json!("not_found"));

    let vue_alice = lister(&db, ALICE).await;
    assert!(contient_nom(&vue_alice, "Privé Alice"));
    assert!(!contient_nom(&vue_alice, "Détourné par Bob"));

    db.destroy().await;
}

#[tokio::test]
async fn ac5_suppression_categorie_d_un_autre_utilisateur_renvoie_404() {
    let db = db_ou_skip!();
    let id = creer(&db, ALICE, "Intouchable Alice", "depense").await;

    let (status, corps) = appel(&db, "DELETE", &format!("/v1/categories/{id}"), BOB, None).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(corps["code"], json!("not_found"));

    let vue_alice = lister(&db, ALICE).await;
    assert!(contient_nom(&vue_alice, "Intouchable Alice"));

    db.destroy().await;
}

#[tokio::test]
async fn ac6_modification_categorie_par_defaut_renvoie_404() {
    let db = db_ou_skip!();
    let liste = lister(&db, ALICE).await;
    let id = id_categorie_par_defaut(&liste);

    let (status, corps) = appel(
        &db,
        "PUT",
        &format!("/v1/categories/{id}"),
        ALICE,
        Some(requete_categorie("Défaut piraté", "depense")),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(corps["code"], json!("not_found"));

    let apres = lister(&db, ALICE).await;
    assert!(!contient_nom(&apres, "Défaut piraté"));

    db.destroy().await;
}

#[tokio::test]
async fn ac6_suppression_categorie_par_defaut_renvoie_404() {
    let db = db_ou_skip!();
    let liste = lister(&db, ALICE).await;
    let avant = total(&liste);
    let id = id_categorie_par_defaut(&liste);

    let (status, corps) = appel(&db, "DELETE", &format!("/v1/categories/{id}"), ALICE, None).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(corps["code"], json!("not_found"));

    let apres = total(&lister(&db, ALICE).await);
    assert_eq!(avant, apres);

    db.destroy().await;
}
