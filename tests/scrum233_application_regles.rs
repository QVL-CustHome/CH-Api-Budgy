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
const ALICE: &str = "qvl-sub-233-alice";
const BOB: &str = "qvl-sub-233-bob";
const CALLBACK_URL: &str = "https://budgy.custhome.app/banque/callback";

macro_rules! db_or_skip {
    () => {
        match DisposableDb::create().await {
            Some(db) => {
                db.migrate().await;
                db
            }
            None => {
                eprintln!("SCRUM-233 : base de test indisponible, test ignoré");
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

async fn creer_regle(db: &DisposableDb, sub: &str, corps: Value) -> (StatusCode, Value) {
    appel(db, "POST", "/v1/categorization-rules", sub, Some(corps)).await
}

async fn creer_regle_ok(
    db: &DisposableDb,
    sub: &str,
    pattern: &str,
    categorie: &str,
    priority: Option<i32>,
) -> String {
    let mut corps = json!({ "label_pattern": pattern, "category_id": categorie });
    if let Some(p) = priority {
        corps["priority"] = json!(p);
    }
    let (status, reponse) = creer_regle(db, sub, corps).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "création de règle attendue en 201"
    );
    reponse["id"].as_str().expect("id de règle").to_string()
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

async fn inserer_transaction(db: &DisposableDb, compte: &BankAccountId, label: &str) -> Uuid {
    let crypto = crypto();
    let inseree = BankTransactionsWriteRepository::enregistrer(
        &SqlxBankTransactionsWriteAdapter::new(db.pool.clone(), crypto),
        NouvelleTransactionBancaire {
            bank_account: compte.clone(),
            external_transaction_id: format!("tx-{}", Uuid::new_v4()),
            status: TransactionStatus::Booked,
            label: label.to_string(),
            amount_cents: -4_590,
            currency: "EUR".to_string(),
            booking_date: None,
            value_date: None,
        },
    )
    .await
    .expect("transaction insérée");
    match inseree {
        ResultatInsertion::Inseree(id) => id.0,
        ResultatInsertion::Doublon => panic!("la transaction devait être insérée"),
    }
}

async fn lire_categorisation(
    db: &DisposableDb,
    tx_id: Uuid,
) -> (Option<Uuid>, String, Option<Uuid>) {
    sqlx::query_as(
        "SELECT category_id, categorization_source, rule_id \
         FROM budgy.bank_transaction WHERE id = $1",
    )
    .bind(tx_id)
    .fetch_one(&db.pool)
    .await
    .expect("lecture de la catégorisation")
}

async fn categoriser_manuellement(
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
    assert_eq!(status, StatusCode::OK);
    assert_eq!(corps["categorization_source"], json!("manual"));
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

fn as_uuid(valeur: &str) -> Uuid {
    Uuid::parse_str(valeur).expect("uuid valide")
}

async fn categorisation_a_l_insert(
    db: &DisposableDb,
    sub: &str,
    pattern: &str,
    label: &str,
) -> (String, String, (Option<Uuid>, String, Option<Uuid>)) {
    let categorie = creer_categorie(db, sub, "Courses").await;
    let regle = creer_regle_ok(db, sub, pattern, &categorie, None).await;
    let compte = semer_compte(db, sub).await;
    let tx = inserer_transaction(db, &compte, label).await;
    let categorisation = lire_categorisation(db, tx).await;
    (categorie, regle, categorisation)
}

#[tokio::test]
async fn ca01_transaction_matchante_recoit_categorie_source_rule_et_rule_id() {
    let db = db_or_skip!();

    let (categorie, regle, (category_id, source, rule_id)) =
        categorisation_a_l_insert(&db, ALICE, "MONOPRIX", "MONOPRIX PARIS 12").await;

    assert_eq!(category_id, Some(as_uuid(&categorie)));
    assert_eq!(source, "rule");
    assert_eq!(rule_id, Some(as_uuid(&regle)));

    db.destroy().await;
}

#[tokio::test]
async fn ca01_pattern_en_debut_de_libelle_matche() {
    let db = db_or_skip!();

    let (categorie, regle, (category_id, source, rule_id)) =
        categorisation_a_l_insert(&db, ALICE, "ACHAT", "ACHAT CARREFOUR MARKET").await;

    assert_eq!(category_id, Some(as_uuid(&categorie)));
    assert_eq!(source, "rule");
    assert_eq!(rule_id, Some(as_uuid(&regle)));

    db.destroy().await;
}

#[tokio::test]
async fn ca01_pattern_au_milieu_du_libelle_matche() {
    let db = db_or_skip!();

    let (categorie, regle, (category_id, source, rule_id)) =
        categorisation_a_l_insert(&db, ALICE, "CARREFOUR", "ACHAT CARREFOUR MARKET").await;

    assert_eq!(category_id, Some(as_uuid(&categorie)));
    assert_eq!(source, "rule");
    assert_eq!(rule_id, Some(as_uuid(&regle)));

    db.destroy().await;
}

#[tokio::test]
async fn ca01_pattern_en_fin_de_libelle_matche() {
    let db = db_or_skip!();

    let (categorie, regle, (category_id, source, rule_id)) =
        categorisation_a_l_insert(&db, ALICE, "MARKET", "ACHAT CARREFOUR MARKET").await;

    assert_eq!(category_id, Some(as_uuid(&categorie)));
    assert_eq!(source, "rule");
    assert_eq!(rule_id, Some(as_uuid(&regle)));

    db.destroy().await;
}

#[tokio::test]
async fn ca01_correspondance_ignore_la_casse_des_deux_cotes() {
    let db = db_or_skip!();

    let (categorie, regle, (category_id, source, rule_id)) =
        categorisation_a_l_insert(&db, ALICE, "CarreFour", "achat CARREFOUR market").await;

    assert_eq!(category_id, Some(as_uuid(&categorie)));
    assert_eq!(source, "rule");
    assert_eq!(rule_id, Some(as_uuid(&regle)));

    db.destroy().await;
}

#[tokio::test]
async fn ca02_aucune_regle_ne_matche_transaction_reste_non_categorisee() {
    let db = db_or_skip!();

    let (_, _, (category_id, source, rule_id)) =
        categorisation_a_l_insert(&db, ALICE, "AMAZON", "MONOPRIX PARIS 12").await;

    assert_eq!(category_id, None);
    assert_eq!(source, "none");
    assert_eq!(rule_id, None);

    db.destroy().await;
}

#[tokio::test]
async fn ca02_sans_aucune_regle_transaction_reste_non_categorisee() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;

    let tx = inserer_transaction(&db, &compte, "MONOPRIX PARIS 12").await;

    let (category_id, source, rule_id) = lire_categorisation(&db, tx).await;
    assert_eq!(category_id, None);
    assert_eq!(source, "none");
    assert_eq!(rule_id, None);

    db.destroy().await;
}

#[tokio::test]
async fn ca03_priorite_la_plus_haute_gagne() {
    let db = db_or_skip!();
    let cat_basse = creer_categorie(&db, ALICE, "Priorité basse").await;
    let cat_haute = creer_categorie(&db, ALICE, "Priorité haute").await;
    creer_regle_ok(&db, ALICE, "CARREFOUR", &cat_basse, Some(1)).await;
    let regle_haute = creer_regle_ok(&db, ALICE, "ACHAT", &cat_haute, Some(10)).await;
    let compte = semer_compte(&db, ALICE).await;

    let tx = inserer_transaction(&db, &compte, "ACHAT CARREFOUR MARKET").await;

    let (category_id, source, rule_id) = lire_categorisation(&db, tx).await;
    assert_eq!(category_id, Some(as_uuid(&cat_haute)));
    assert_eq!(source, "rule");
    assert_eq!(rule_id, Some(as_uuid(&regle_haute)));

    db.destroy().await;
}

#[tokio::test]
async fn ca03_egalite_de_priorite_la_plus_recente_gagne() {
    let db = db_or_skip!();
    let cat_ancienne = creer_categorie(&db, ALICE, "Règle ancienne").await;
    let cat_recente = creer_categorie(&db, ALICE, "Règle récente").await;
    creer_regle_ok(&db, ALICE, "ACHAT", &cat_ancienne, Some(5)).await;
    let regle_recente = creer_regle_ok(&db, ALICE, "CARREFOUR", &cat_recente, Some(5)).await;
    let compte = semer_compte(&db, ALICE).await;

    let tx = inserer_transaction(&db, &compte, "ACHAT CARREFOUR MARKET").await;

    let (category_id, source, rule_id) = lire_categorisation(&db, tx).await;
    assert_eq!(category_id, Some(as_uuid(&cat_recente)));
    assert_eq!(source, "rule");
    assert_eq!(rule_id, Some(as_uuid(&regle_recente)));

    db.destroy().await;
}

#[tokio::test]
async fn ca04_creation_regle_recategorise_l_historique_non_categorise() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let tx = inserer_transaction(&db, &compte, "MONOPRIX PARIS 12").await;

    let avant = lire_categorisation(&db, tx).await;
    assert_eq!(
        avant.1, "none",
        "l'historique doit d'abord être non catégorisé"
    );

    let categorie = creer_categorie(&db, ALICE, "Courses").await;
    let (status, _) = creer_regle(
        &db,
        ALICE,
        json!({ "label_pattern": "MONOPRIX", "category_id": categorie }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let regle = regle_unique(&db, ALICE).await;

    let (category_id, source, rule_id) = lire_categorisation(&db, tx).await;
    assert_eq!(category_id, Some(as_uuid(&categorie)));
    assert_eq!(source, "rule");
    assert_eq!(rule_id, Some(regle));

    db.destroy().await;
}

#[tokio::test]
async fn ca05_retroactif_ne_touche_pas_une_transaction_manuelle() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let tx = inserer_transaction(&db, &compte, "MONOPRIX PARIS 12").await;
    let cat_manuelle = creer_categorie(&db, ALICE, "Choix manuel").await;
    categoriser_manuellement(&db, ALICE, &compte, tx, &cat_manuelle).await;

    let cat_regle = creer_categorie(&db, ALICE, "Courses").await;
    let (status, _) = creer_regle(
        &db,
        ALICE,
        json!({ "label_pattern": "MONOPRIX", "category_id": cat_regle }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (category_id, source, _) = lire_categorisation(&db, tx).await;
    assert_eq!(category_id, Some(as_uuid(&cat_manuelle)));
    assert_eq!(source, "manual");

    db.destroy().await;
}

#[tokio::test]
async fn ca06_retroactif_ne_reecrit_pas_une_transaction_deja_categorisee_par_regle() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let tx = inserer_transaction(&db, &compte, "MONOPRIX PARIS 12").await;

    let cat_premiere = creer_categorie(&db, ALICE, "Première règle").await;
    let (status_1, _) = creer_regle(
        &db,
        ALICE,
        json!({ "label_pattern": "MONOPRIX", "category_id": cat_premiere, "priority": 1 }),
    )
    .await;
    assert_eq!(status_1, StatusCode::CREATED);
    let (apres_1, source_1, regle_1) = lire_categorisation(&db, tx).await;
    assert_eq!(apres_1, Some(as_uuid(&cat_premiere)));
    assert_eq!(source_1, "rule");

    let cat_seconde = creer_categorie(&db, ALICE, "Seconde règle").await;
    let (status_2, _) = creer_regle(
        &db,
        ALICE,
        json!({ "label_pattern": "MONOPRIX", "category_id": cat_seconde, "priority": 100 }),
    )
    .await;
    assert_eq!(status_2, StatusCode::CREATED);

    let (category_id, source, rule_id) = lire_categorisation(&db, tx).await;
    assert_eq!(category_id, Some(as_uuid(&cat_premiere)));
    assert_eq!(source, "rule");
    assert_eq!(rule_id, regle_1);

    db.destroy().await;
}

#[tokio::test]
async fn ca07_creation_regle_n_affecte_que_les_transactions_de_l_owner() {
    let db = db_or_skip!();
    let compte_alice = semer_compte(&db, ALICE).await;
    let tx_alice = inserer_transaction(&db, &compte_alice, "MONOPRIX PARIS 12").await;
    let compte_bob = semer_compte(&db, BOB).await;
    let tx_bob = inserer_transaction(&db, &compte_bob, "MONOPRIX PARIS 12").await;

    let categorie_alice = creer_categorie(&db, ALICE, "Courses").await;
    let (status, _) = creer_regle(
        &db,
        ALICE,
        json!({ "label_pattern": "MONOPRIX", "category_id": categorie_alice }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (cat_a, source_a, rule_a) = lire_categorisation(&db, tx_alice).await;
    assert_eq!(cat_a, Some(as_uuid(&categorie_alice)));
    assert_eq!(source_a, "rule");
    assert!(rule_a.is_some());

    let (cat_b, source_b, rule_b) = lire_categorisation(&db, tx_bob).await;
    assert_eq!(
        cat_b, None,
        "la transaction de B ne doit jamais être touchée"
    );
    assert_eq!(source_b, "none");
    assert_eq!(rule_b, None);

    db.destroy().await;
}

#[tokio::test]
async fn ca08_retroactif_laisse_intactes_les_transactions_non_matchantes() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let tx = inserer_transaction(&db, &compte, "MONOPRIX PARIS 12").await;

    let categorie = creer_categorie(&db, ALICE, "Loisirs").await;
    let (status, _) = creer_regle(
        &db,
        ALICE,
        json!({ "label_pattern": "AMAZON", "category_id": categorie }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (category_id, source, rule_id) = lire_categorisation(&db, tx).await;
    assert_eq!(category_id, None);
    assert_eq!(source, "none");
    assert_eq!(rule_id, None);

    db.destroy().await;
}

#[tokio::test]
async fn ca09_creation_regle_renvoie_201_et_persiste_meme_sans_historique() {
    let db = db_or_skip!();
    let categorie = creer_categorie(&db, ALICE, "Courses").await;

    let (status, corps) = creer_regle(
        &db,
        ALICE,
        json!({ "label_pattern": "MONOPRIX", "category_id": categorie }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert!(corps["id"].is_string());
    assert_eq!(regles_persistees(&db, ALICE).await, 1);

    db.destroy().await;
}

async fn regle_unique(db: &DisposableDb, owner: &str) -> Uuid {
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM budgy.regles_categorisation WHERE owner_id = $1")
        .bind(owner)
        .fetch_one(&db.pool)
        .await
        .expect("règle unique du propriétaire")
}
