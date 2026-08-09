//! Virements internes : un mouvement entre deux comptes du même propriétaire
//! n'est ni une dépense ni un revenu. Sans appariement, il est compté deux fois
//! (dépense côté émetteur, revenu côté destinataire) et gonfle le reste à
//! dépenser comme le prévisionnel.

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
use ch_api_budgy::domain::transfert_interne::CATEGORIE_VIREMENTS_INTERNES;
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
const ALICE: &str = "qvl-sub-transferts-alice";
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
                    "Transferts internes ignorés : variable {} absente (Postgres jetable requis)",
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
    label: &str,
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
            label: label.to_string(),
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

async fn reconcilier(db: &DisposableDb, sub: &str) {
    let (status, corps) = appel(db, "POST", "/v1/transactions/recategoriser", sub, None).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "réconciliation attendue OK : {corps}"
    );
}

async fn depenses_du_mois(db: &DisposableDb, sub: &str) -> Value {
    let (status, corps) = appel(
        db,
        "GET",
        &format!("/v1/expenses/by-category?month={MOIS}"),
        sub,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{corps}");
    corps
}

async fn est_transfert_interne(db: &DisposableDb, transaction: Uuid) -> bool {
    let (marque,): (bool,) =
        sqlx::query_as("SELECT is_internal_transfer FROM budgy.bank_transaction WHERE id = $1")
            .bind(transaction)
            .fetch_one(&db.pool)
            .await
            .expect("lecture du marquage");
    marque
}

async fn categorisation(db: &DisposableDb, transaction: Uuid) -> (Option<Uuid>, String) {
    sqlx::query_as(
        "SELECT category_id, categorization_source FROM budgy.bank_transaction WHERE id = $1",
    )
    .bind(transaction)
    .fetch_one(&db.pool)
    .await
    .expect("lecture de la catégorisation")
}

fn jour(annee: i32, mois: u32, jour: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(annee, mois, jour).unwrap()
}

#[tokio::test]
async fn un_virement_entre_deux_comptes_est_exclu_des_depenses() {
    let db = db_or_skip!();
    let courant = semer_compte(&db, ALICE).await;
    let epargne = semer_compte(&db, ALICE).await;

    // Les deux faces du même virement, datées différemment par chaque banque.
    let debit = semer_transaction(
        &db,
        &courant,
        "VIREMENT BOURSORAMA BOULOGNE",
        -30_000,
        jour(2026, 7, 6),
    )
    .await;
    let credit =
        semer_transaction(&db, &epargne, "ALIMENTATION CB", 30_000, jour(2026, 7, 3)).await;
    // Une vraie dépense, qui doit rester comptée.
    semer_transaction(&db, &courant, "INTERMARCHE", -8_580, jour(2026, 7, 10)).await;

    reconcilier(&db, ALICE).await;

    assert!(est_transfert_interne(&db, debit).await, "débit apparié");
    assert!(est_transfert_interne(&db, credit).await, "crédit apparié");

    let corps = depenses_du_mois(&db, ALICE).await;
    assert_eq!(
        corps["total_cents"],
        json!(8_580),
        "seule la vraie dépense doit compter : {corps}"
    );

    db.destroy().await;
}

#[tokio::test]
async fn le_credit_d_un_virement_n_est_pas_categorise_en_salaire() {
    let db = db_or_skip!();
    let courant = semer_compte(&db, ALICE).await;
    let epargne = semer_compte(&db, ALICE).await;

    semer_transaction(&db, &courant, "VIREMENT SORTANT", -20_000, jour(2026, 7, 6)).await;
    let credit =
        semer_transaction(&db, &epargne, "VIREMENT ENTRANT", 20_000, jour(2026, 7, 6)).await;

    reconcilier(&db, ALICE).await;

    let (categorie, source) = categorisation(&db, credit).await;
    assert_eq!(
        categorie, None,
        "un virement interne n'est pas un revenu : aucune catégorie ne doit être posée"
    );
    assert_eq!(source, "none");

    db.destroy().await;
}

#[tokio::test]
async fn une_vraie_depense_et_un_vrai_salaire_ne_sont_pas_apparies() {
    let db = db_or_skip!();
    let courant = semer_compte(&db, ALICE).await;
    let autre = semer_compte(&db, ALICE).await;

    let depense = semer_transaction(&db, &courant, "INTERMARCHE", -8_580, jour(2026, 7, 10)).await;
    let salaire = semer_transaction(&db, &autre, "BLUE SOFT", 120_926, jour(2026, 7, 30)).await;

    reconcilier(&db, ALICE).await;

    assert!(
        !est_transfert_interne(&db, depense).await,
        "des montants différents ne forment pas un virement"
    );
    assert!(!est_transfert_interne(&db, salaire).await);

    let corps = depenses_du_mois(&db, ALICE).await;
    assert_eq!(corps["total_cents"], json!(8_580), "{corps}");

    db.destroy().await;
}

#[tokio::test]
async fn ranger_une_transaction_dans_virements_internes_l_exclut_des_depenses() {
    let db = db_or_skip!();
    let courant = semer_compte(&db, ALICE).await;

    // Virement vers un compte NON rattaché à Budgy : aucun appariement possible,
    // l'utilisateur le range donc lui-même dans « Virements internes ».
    let virement = semer_transaction(
        &db,
        &courant,
        "WEB QUEVAL MARTIN",
        -20_000,
        jour(2026, 7, 31),
    )
    .await;
    semer_transaction(&db, &courant, "INTERMARCHE", -8_580, jour(2026, 7, 10)).await;

    let (status, corps) = appel(
        &db,
        "PUT",
        &format!(
            "/v1/accounts/{}/transactions/{virement}/category",
            courant.0
        ),
        ALICE,
        Some(json!({ "category_id": CATEGORIE_VIREMENTS_INTERNES.to_string() })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "catégorisation attendue OK : {corps}"
    );

    assert!(
        est_transfert_interne(&db, virement).await,
        "la catégorie système doit exclure la transaction des calculs"
    );

    let corps = depenses_du_mois(&db, ALICE).await;
    assert_eq!(
        corps["total_cents"],
        json!(8_580),
        "le virement manuel ne doit plus compter comme une dépense : {corps}"
    );

    db.destroy().await;
}
