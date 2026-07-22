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
use chrono::{NaiveDate, TimeZone, Utc};
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
const OWNER: &str = "owner-scrum-240";
const AUTRE_OWNER: &str = "owner-scrum-240-intrus";

macro_rules! db_or_skip {
    () => {
        match DisposableDb::create().await {
            Some(db) => {
                db.migrate().await;
                db
            }
            None => {
                eprintln!(
                    "SCRUM-240 ignoré : variable {} absente (Postgres jetable requis)",
                    common::ENV_ADMIN_URL
                );
                return;
            }
        }
    };
}

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
        bank_callback_url: "https://budgy.custhome.app/banque/callback".to_string(),
        db: db.pool.clone(),
        crypto: crypto.clone(),
        jwt: Arc::new(JwtService::from_secret(TEST_SECRET, ISSUER, AUDIENCE)),
    }
}

async fn get(
    db: &DisposableDb,
    crypto: &Arc<CryptoService>,
    owner: &str,
    uri: &str,
) -> (StatusCode, String) {
    let request = Request::builder()
        .method("GET")
        .uri(uri)
        .header("Authorization", bearer(owner))
        .body(Body::empty())
        .unwrap();
    let response = router(state(db, crypto)).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

fn body_json(corps: &str) -> Value {
    serde_json::from_str(corps)
        .unwrap_or_else(|_| panic!("le corps n'est pas un JSON valide : {corps}"))
}

fn labels(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["label"].as_str().unwrap().to_string())
        .collect()
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

struct SeedTransaction<'a> {
    db: &'a DisposableDb,
    crypto: &'a Arc<CryptoService>,
    compte: &'a BankAccountId,
    external_id: &'a str,
    label: &'a str,
    amount_cents: i64,
    booking_date: Option<NaiveDate>,
}

async fn transaction(seed: SeedTransaction<'_>) -> Uuid {
    let inseree = BankTransactionsWriteRepository::enregistrer(
        &SqlxBankTransactionsWriteAdapter::new(seed.db.pool.clone(), seed.crypto.clone()),
        NouvelleTransactionBancaire {
            bank_account: seed.compte.clone(),
            external_transaction_id: seed.external_id.to_string(),
            status: TransactionStatus::Booked,
            label: seed.label.to_string(),
            amount_cents: seed.amount_cents,
            currency: "EUR".to_string(),
            booking_date: seed.booking_date,
            value_date: seed.booking_date,
        },
    )
    .await
    .expect("transaction enregistrée");
    match inseree {
        ResultatInsertion::Inseree(id) => id.0,
        ResultatInsertion::Doublon => panic!("la transaction devait être insérée"),
    }
}

async fn premiere_categorie(db: &DisposableDb) -> Uuid {
    sqlx::query_scalar("SELECT id FROM budgy.category ORDER BY kind, name LIMIT 1")
        .fetch_one(&db.pool)
        .await
        .expect("au moins une catégorie via le seed")
}

async fn categoriser(db: &DisposableDb, tx_id: Uuid, categorie: Uuid) {
    sqlx::query(
        "UPDATE budgy.bank_transaction \
         SET category_id = $2, categorization_source = 'manual' WHERE id = $1",
    )
    .bind(tx_id)
    .bind(categorie)
    .execute(&db.pool)
    .await
    .expect("catégorisation de la transaction");
}

fn jour(annee: i32, mois: u32, jour: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(annee, mois, jour).unwrap()
}

#[tokio::test]
async fn ca01_liste_expose_date_libelle_montant_et_categorie() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = consent(&db, &crypto, OWNER).await;
    let compte = compte(&db, &crypto, OWNER, consent, "FR7630006000011234567890189").await;
    let categorie = premiere_categorie(&db).await;
    let tx = transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-ca01",
        label: "MONOPRIX PARIS",
        amount_cents: -2_599,
        booking_date: Some(jour(2026, 6, 10)),
    })
    .await;
    categoriser(&db, tx, categorie).await;

    let (status, corps) = get(&db, &crypto, OWNER, "/v1/transactions").await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], json!(1));
    let item = &body["data"][0];
    assert_eq!(item["label"], json!("MONOPRIX PARIS"));
    assert_eq!(item["booking_date"], json!("2026-06-10"));
    assert_eq!(item["amount_cents"], json!(-2_599));
    assert!(item["amount_cents"].is_i64());
    assert_eq!(item["category_id"], json!(categorie.to_string()));

    db.destroy().await;
}

#[tokio::test]
async fn ca02_filtre_account_id_n_expose_que_le_compte_cible() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent_a = consent(&db, &crypto, OWNER).await;
    let compte_a = compte(
        &db,
        &crypto,
        OWNER,
        consent_a,
        "FR7630006000011234567890189",
    )
    .await;
    let consent_b = consent(&db, &crypto, OWNER).await;
    let compte_b = compte(
        &db,
        &crypto,
        OWNER,
        consent_b,
        "FR7610107001011234567890129",
    )
    .await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte_a,
        external_id: "tx-a",
        label: "COMPTE_A",
        amount_cents: -100,
        booking_date: Some(jour(2026, 6, 1)),
    })
    .await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte_b,
        external_id: "tx-b",
        label: "COMPTE_B",
        amount_cents: -200,
        booking_date: Some(jour(2026, 6, 2)),
    })
    .await;

    let (status, corps) = get(
        &db,
        &crypto,
        OWNER,
        &format!("/v1/transactions?account_id={}", compte_a.0),
    )
    .await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], json!(1));
    assert_eq!(labels(&body), vec!["COMPTE_A"]);

    db.destroy().await;
}

#[tokio::test]
async fn ca02_filtre_category_id_n_expose_que_la_categorie_cible() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = consent(&db, &crypto, OWNER).await;
    let compte = compte(&db, &crypto, OWNER, consent, "FR7630006000011234567890189").await;
    let categorie = premiere_categorie(&db).await;
    let categorisee = transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-cat",
        label: "CATEGORISEE",
        amount_cents: -300,
        booking_date: Some(jour(2026, 6, 3)),
    })
    .await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-nocat",
        label: "SANS_CATEGORIE",
        amount_cents: -400,
        booking_date: Some(jour(2026, 6, 4)),
    })
    .await;
    categoriser(&db, categorisee, categorie).await;

    let (status, corps) = get(
        &db,
        &crypto,
        OWNER,
        &format!("/v1/transactions?category_id={categorie}"),
    )
    .await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], json!(1));
    assert_eq!(labels(&body), vec!["CATEGORISEE"]);

    db.destroy().await;
}

#[tokio::test]
async fn ca02_filtre_periode_from_to_ne_garde_que_l_intervalle() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = consent(&db, &crypto, OWNER).await;
    let compte = compte(&db, &crypto, OWNER, consent, "FR7630006000011234567890189").await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-avant",
        label: "AVANT",
        amount_cents: -100,
        booking_date: Some(jour(2026, 1, 15)),
    })
    .await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-dedans",
        label: "DEDANS",
        amount_cents: -200,
        booking_date: Some(jour(2026, 6, 15)),
    })
    .await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-apres",
        label: "APRES",
        amount_cents: -300,
        booking_date: Some(jour(2026, 12, 15)),
    })
    .await;

    let (status, corps) = get(
        &db,
        &crypto,
        OWNER,
        "/v1/transactions?from=2026-06-01&to=2026-06-30",
    )
    .await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], json!(1));
    assert_eq!(labels(&body), vec!["DEDANS"]);

    db.destroy().await;
}

#[tokio::test]
async fn ca02_filtre_type_credit_n_expose_que_les_entrees() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = consent(&db, &crypto, OWNER).await;
    let compte = compte(&db, &crypto, OWNER, consent, "FR7630006000011234567890189").await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-salaire",
        label: "SALAIRE",
        amount_cents: 250_000,
        booking_date: Some(jour(2026, 6, 1)),
    })
    .await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-loyer",
        label: "LOYER",
        amount_cents: -90_000,
        booking_date: Some(jour(2026, 6, 2)),
    })
    .await;

    let (status, corps) = get(&db, &crypto, OWNER, "/v1/transactions?type=credit").await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], json!(1));
    assert_eq!(labels(&body), vec!["SALAIRE"]);
    assert!(body["data"][0]["amount_cents"].as_i64().unwrap() > 0);

    db.destroy().await;
}

#[tokio::test]
async fn ca02_filtre_type_debit_n_expose_que_les_sorties() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = consent(&db, &crypto, OWNER).await;
    let compte = compte(&db, &crypto, OWNER, consent, "FR7630006000011234567890189").await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-salaire",
        label: "SALAIRE",
        amount_cents: 250_000,
        booking_date: Some(jour(2026, 6, 1)),
    })
    .await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-loyer",
        label: "LOYER",
        amount_cents: -90_000,
        booking_date: Some(jour(2026, 6, 2)),
    })
    .await;

    let (status, corps) = get(&db, &crypto, OWNER, "/v1/transactions?type=debit").await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], json!(1));
    assert_eq!(labels(&body), vec!["LOYER"]);
    assert!(body["data"][0]["amount_cents"].as_i64().unwrap() < 0);

    db.destroy().await;
}

#[tokio::test]
async fn ca03_tri_par_date_ascendant_et_descendant() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = consent(&db, &crypto, OWNER).await;
    let compte = compte(&db, &crypto, OWNER, consent, "FR7630006000011234567890189").await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-1",
        label: "ANCIEN",
        amount_cents: -100,
        booking_date: Some(jour(2026, 1, 1)),
    })
    .await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-2",
        label: "MILIEU",
        amount_cents: -200,
        booking_date: Some(jour(2026, 3, 15)),
    })
    .await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-3",
        label: "RECENT",
        amount_cents: -300,
        booking_date: Some(jour(2026, 6, 1)),
    })
    .await;

    let (_, asc) = get(&db, &crypto, OWNER, "/v1/transactions?sort=date&order=asc").await;
    assert_eq!(labels(&body_json(&asc)), vec!["ANCIEN", "MILIEU", "RECENT"]);

    let (_, desc) = get(&db, &crypto, OWNER, "/v1/transactions?sort=date&order=desc").await;
    assert_eq!(
        labels(&body_json(&desc)),
        vec!["RECENT", "MILIEU", "ANCIEN"]
    );

    db.destroy().await;
}

#[tokio::test]
async fn ca03_tri_par_montant_ascendant_et_descendant() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = consent(&db, &crypto, OWNER).await;
    let compte = compte(&db, &crypto, OWNER, consent, "FR7630006000011234567890189").await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-bas",
        label: "BAS",
        amount_cents: -900,
        booking_date: Some(jour(2026, 6, 1)),
    })
    .await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-milieu",
        label: "MILIEU",
        amount_cents: 100,
        booking_date: Some(jour(2026, 6, 2)),
    })
    .await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte,
        external_id: "tx-haut",
        label: "HAUT",
        amount_cents: 5_000,
        booking_date: Some(jour(2026, 6, 3)),
    })
    .await;

    let (_, asc) = get(
        &db,
        &crypto,
        OWNER,
        "/v1/transactions?sort=amount&order=asc",
    )
    .await;
    assert_eq!(labels(&body_json(&asc)), vec!["BAS", "MILIEU", "HAUT"]);

    let (_, desc) = get(
        &db,
        &crypto,
        OWNER,
        "/v1/transactions?sort=amount&order=desc",
    )
    .await;
    assert_eq!(labels(&body_json(&desc)), vec!["HAUT", "MILIEU", "BAS"]);

    db.destroy().await;
}

#[tokio::test]
async fn pagination_limit_offset_et_total_reste_le_total_global() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent = consent(&db, &crypto, OWNER).await;
    let compte = compte(&db, &crypto, OWNER, consent, "FR7630006000011234567890189").await;
    for index in 0..5 {
        transaction(SeedTransaction {
            db: &db,
            crypto: &crypto,
            compte: &compte,
            external_id: &format!("tx-{index}"),
            label: &format!("TX_{index}"),
            amount_cents: -(100 * (index + 1)),
            booking_date: Some(jour(2026, 6, index as u32 + 1)),
        })
        .await;
    }

    let (status, page1) = get(&db, &crypto, OWNER, "/v1/transactions?limit=2&offset=0").await;
    let body1 = body_json(&page1);
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body1["total"], json!(5));
    assert_eq!(body1["data"].as_array().unwrap().len(), 2);

    let (_, page3) = get(&db, &crypto, OWNER, "/v1/transactions?limit=2&offset=4").await;
    let body3 = body_json(&page3);
    assert_eq!(body3["total"], json!(5));
    assert_eq!(body3["data"].as_array().unwrap().len(), 1);

    db.destroy().await;
}

#[tokio::test]
async fn securite_type_invalide_est_rejete_proprement() {
    let db = db_or_skip!();
    let crypto = crypto();

    let (status, corps) = get(&db, &crypto, OWNER, "/v1/transactions?type=peu_importe").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body_json(&corps)["code"], json!("bad_request"));

    db.destroy().await;
}

#[tokio::test]
async fn securite_sort_invalide_est_rejete_proprement() {
    let db = db_or_skip!();
    let crypto = crypto();

    let (status, corps) = get(&db, &crypto, OWNER, "/v1/transactions?sort=libelle").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body_json(&corps)["code"], json!("bad_request"));

    db.destroy().await;
}

#[tokio::test]
async fn securite_order_invalide_est_rejete_proprement() {
    let db = db_or_skip!();
    let crypto = crypto();

    let (status, corps) = get(&db, &crypto, OWNER, "/v1/transactions?order=croissant").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body_json(&corps)["code"], json!("bad_request"));

    db.destroy().await;
}

#[tokio::test]
async fn securite_limit_zero_et_au_dela_du_max_sont_rejetes() {
    let db = db_or_skip!();
    let crypto = crypto();

    let (zero, _) = get(&db, &crypto, OWNER, "/v1/transactions?limit=0").await;
    assert_eq!(zero, StatusCode::BAD_REQUEST);

    let (trop, _) = get(&db, &crypto, OWNER, "/v1/transactions?limit=201").await;
    assert_eq!(trop, StatusCode::BAD_REQUEST);

    db.destroy().await;
}

#[tokio::test]
async fn securite_from_posterieur_a_to_est_rejete() {
    let db = db_or_skip!();
    let crypto = crypto();

    let (status, corps) = get(
        &db,
        &crypto,
        OWNER,
        "/v1/transactions?from=2026-12-31&to=2026-01-01",
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body_json(&corps)["code"], json!("bad_request"));

    db.destroy().await;
}

#[tokio::test]
async fn securite_tentative_injection_sql_dans_type_ne_declenche_pas_d_erreur_serveur() {
    let db = db_or_skip!();
    let crypto = crypto();

    let (status, corps) = get(
        &db,
        &crypto,
        OWNER,
        "/v1/transactions?type=credit%27%20OR%20%271%27%3D%271",
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!corps.to_lowercase().contains("sql"));
    assert!(!corps.to_lowercase().contains("syntax"));

    db.destroy().await;
}

#[tokio::test]
async fn idor_un_owner_ne_voit_que_ses_propres_transactions() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent_owner = consent(&db, &crypto, OWNER).await;
    let compte_owner = compte(
        &db,
        &crypto,
        OWNER,
        consent_owner,
        "FR7630006000011234567890189",
    )
    .await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte_owner,
        external_id: "tx-owner",
        label: "A_MOI",
        amount_cents: -100,
        booking_date: Some(jour(2026, 6, 1)),
    })
    .await;
    let consent_intrus = consent(&db, &crypto, AUTRE_OWNER).await;
    let compte_intrus = compte(
        &db,
        &crypto,
        AUTRE_OWNER,
        consent_intrus,
        "FR7610107001011234567890129",
    )
    .await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte_intrus,
        external_id: "tx-intrus",
        label: "SECRET_AUTRUI",
        amount_cents: -999,
        booking_date: Some(jour(2026, 6, 2)),
    })
    .await;

    let (status, corps) = get(&db, &crypto, OWNER, "/v1/transactions").await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], json!(1));
    assert_eq!(labels(&body), vec!["A_MOI"]);
    assert!(!corps.contains("SECRET_AUTRUI"));

    db.destroy().await;
}

#[tokio::test]
async fn idor_filtrer_sur_le_compte_d_autrui_n_expose_rien() {
    let db = db_or_skip!();
    let crypto = crypto();
    let consent_intrus = consent(&db, &crypto, AUTRE_OWNER).await;
    let compte_intrus = compte(
        &db,
        &crypto,
        AUTRE_OWNER,
        consent_intrus,
        "FR7610107001011234567890129",
    )
    .await;
    transaction(SeedTransaction {
        db: &db,
        crypto: &crypto,
        compte: &compte_intrus,
        external_id: "tx-intrus",
        label: "SECRET_AUTRUI",
        amount_cents: -999,
        booking_date: Some(jour(2026, 6, 2)),
    })
    .await;

    let (status, corps) = get(
        &db,
        &crypto,
        OWNER,
        &format!("/v1/transactions?account_id={}", compte_intrus.0),
    )
    .await;
    let body = body_json(&corps);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], json!(0));
    assert!(body["data"].as_array().unwrap().is_empty());
    assert!(!corps.contains("SECRET_AUTRUI"));

    db.destroy().await;
}
