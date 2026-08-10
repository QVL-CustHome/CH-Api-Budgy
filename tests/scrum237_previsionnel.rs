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
use ch_api_budgy::repository::enveloppes::SqlxEnveloppesRepository;
use ch_api_budgy::repository::preferences::SqlxPreferencesRepository;
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
const ALICE: &str = "qvl-sub-237-alice";
const BOB: &str = "qvl-sub-237-bob";
const CALLBACK_URL: &str = "https://budgy.custhome.app/banque/callback";
const MOIS_PREVU: &str = "2026-07";

macro_rules! db_or_skip {
    () => {
        match DisposableDb::create().await {
            Some(db) => {
                db.migrate().await;
                db
            }
            None => {
                eprintln!(
                    "SCRUM-237 ignoré : variable {} absente ou Postgres jetable indisponible",
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
            expires_at: Some(
                NaiveDate::from_ymd_opt(2030, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc(),
            ),
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

async fn recalculer_recurrences(db: &DisposableDb, sub: &str) -> u64 {
    let crypto = crypto();
    SqlxBankTransactionsWriteAdapter::new(db.pool.clone(), crypto)
        .recalculer_recurrences(&ProprietaireId(sub.to_string()))
        .await
        .expect("recalcul des récurrences")
}

async fn categorie_id_par_nom(db: &DisposableDb, sub: &str, nom: &str) -> String {
    let (status, corps) = appel(db, "GET", "/v1/categories", sub, None).await;
    assert_eq!(status, StatusCode::OK);
    corps["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == json!(nom))
        .unwrap_or_else(|| panic!("catégorie « {nom} » absente : {corps}"))["id"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn categoriser(
    db: &DisposableDb,
    sub: &str,
    compte: &BankAccountId,
    tx: Uuid,
    categorie: &str,
) {
    let (status, corps) = appel(
        db,
        "PUT",
        &format!("/v1/accounts/{}/transactions/{tx}/category", compte.0),
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
        "budget attendu CREATED : {corps}"
    );
}

async fn forecast(db: &DisposableDb, sub: &str, mois: &str) -> (StatusCode, Value) {
    appel(db, "GET", &format!("/v1/forecast?month={mois}"), sub, None).await
}

async fn semer_revenu_recurrent(
    db: &DisposableDb,
    compte: &BankAccountId,
    montant_cents: i64,
) -> [Uuid; 3] {
    [
        semer_transaction(
            db,
            compte,
            "SALAIRE MENSUEL",
            montant_cents,
            jour(2026, 4, 15),
        )
        .await,
        semer_transaction(
            db,
            compte,
            "SALAIRE MENSUEL",
            montant_cents,
            jour(2026, 5, 15),
        )
        .await,
        semer_transaction(
            db,
            compte,
            "SALAIRE MENSUEL",
            montant_cents,
            jour(2026, 6, 15),
        )
        .await,
    ]
}

async fn semer_depense_recurrente(
    db: &DisposableDb,
    compte: &BankAccountId,
    montant_cents: i64,
) -> [Uuid; 3] {
    let montant_cents = -montant_cents.abs();
    [
        semer_transaction(
            db,
            compte,
            "LOYER APPARTEMENT",
            montant_cents,
            jour(2026, 4, 3),
        )
        .await,
        semer_transaction(
            db,
            compte,
            "LOYER APPARTEMENT",
            montant_cents,
            jour(2026, 5, 3),
        )
        .await,
        semer_transaction(
            db,
            compte,
            "LOYER APPARTEMENT",
            montant_cents,
            jour(2026, 6, 3),
        )
        .await,
    ]
}

fn jour(annee: i32, mois: u32, jour: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(annee, mois, jour).unwrap()
}

fn entier(corps: &Value, cle: &str) -> i64 {
    corps[cle]
        .as_i64()
        .unwrap_or_else(|| panic!("{cle} doit être un entier en centimes : {corps}"))
}

#[tokio::test]
async fn ca01_solde_projete_part_du_solde_actuel() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let salaire = categorie_id_par_nom(&db, ALICE, "Salaire").await;
    let loyer = categorie_id_par_nom(&db, ALICE, "Loyer").await;
    let courses = categorie_id_par_nom(&db, ALICE, "Courses").await;

    let revenus = semer_revenu_recurrent(&db, &compte, 200_000).await;
    let depenses = semer_depense_recurrente(&db, &compte, 80_000).await;
    recalculer_recurrences(&db, ALICE).await;
    for tx in revenus {
        categoriser(&db, ALICE, &compte, tx, &salaire).await;
    }
    for tx in depenses {
        categoriser(&db, ALICE, &compte, tx, &loyer).await;
    }
    definir_budget(&db, ALICE, &courses, 30_000, MOIS_PREVU).await;

    let (status, corps) = forecast(&db, ALICE, MOIS_PREVU).await;

    assert_eq!(status, StatusCode::OK, "{corps}");
    assert_eq!(corps["month"], json!(MOIS_PREVU));
    assert_eq!(corps["donnees_suffisantes"], json!(true), "{corps}");

    let solde_actuel_cents = entier(&corps, "solde_actuel_cents");
    let revenus_cents = entier(&corps, "revenus_restants_cents");
    let depenses_cents = entier(&corps, "depenses_restantes_cents");
    let solde_cents = entier(&corps, "solde_previsionnel_cents");

    assert_eq!(
        solde_cents,
        solde_actuel_cents + revenus_cents - depenses_cents,
        "le solde projeté doit valoir solde actuel + revenus restants - dépenses restantes : {corps}"
    );
    assert_eq!(revenus_cents, 200_000, "{corps}");
    assert_eq!(depenses_cents, 80_000, "{corps}");
    // Un budget mensuel par catégorie ne pèse plus sur la projection : elle
    // porte sur la trésorerie, pas sur le respect d'une enveloppe.
    assert_eq!(solde_cents, solde_actuel_cents + 120_000, "{corps}");

    db.destroy().await;
}

#[tokio::test]
async fn ca01_le_solde_projete_suit_revenus_et_depenses_restants() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let salaire = categorie_id_par_nom(&db, ALICE, "Salaire").await;
    let loyer = categorie_id_par_nom(&db, ALICE, "Loyer").await;

    let revenus = semer_revenu_recurrent(&db, &compte, 210_000).await;
    let depenses = semer_depense_recurrente(&db, &compte, 90_000).await;
    recalculer_recurrences(&db, ALICE).await;
    for tx in revenus {
        categoriser(&db, ALICE, &compte, tx, &salaire).await;
    }
    for tx in depenses {
        categoriser(&db, ALICE, &compte, tx, &loyer).await;
    }

    let (status, corps) = forecast(&db, ALICE, MOIS_PREVU).await;

    assert_eq!(status, StatusCode::OK, "{corps}");
    let revenus_cents = entier(&corps, "revenus_restants_cents");
    let depenses_cents = entier(&corps, "depenses_restantes_cents");
    assert_eq!(
        entier(&corps, "solde_previsionnel_cents"),
        entier(&corps, "solde_actuel_cents") + revenus_cents - depenses_cents,
        "{corps}"
    );
    assert_eq!(revenus_cents, 210_000, "{corps}");
    assert_eq!(depenses_cents, 90_000, "{corps}");

    db.destroy().await;
}

#[tokio::test]
async fn ca01_revenus_recurrents_seuls_donnent_un_solde_positif() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let salaire = categorie_id_par_nom(&db, ALICE, "Salaire").await;

    let revenus = semer_revenu_recurrent(&db, &compte, 180_000).await;
    recalculer_recurrences(&db, ALICE).await;
    for tx in revenus {
        categoriser(&db, ALICE, &compte, tx, &salaire).await;
    }

    let (status, corps) = forecast(&db, ALICE, MOIS_PREVU).await;

    assert_eq!(status, StatusCode::OK, "{corps}");
    assert_eq!(entier(&corps, "revenus_restants_cents"), 180_000, "{corps}");
    assert_eq!(entier(&corps, "depenses_restantes_cents"), 0, "{corps}");
    assert_eq!(
        entier(&corps, "solde_previsionnel_cents"),
        180_000,
        "{corps}"
    );

    db.destroy().await;
}

#[tokio::test]
async fn ca01_depenses_recurrentes_seules_donnent_un_solde_negatif() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let loyer = categorie_id_par_nom(&db, ALICE, "Loyer").await;

    let depenses = semer_depense_recurrente(&db, &compte, 95_000).await;
    recalculer_recurrences(&db, ALICE).await;
    for tx in depenses {
        categoriser(&db, ALICE, &compte, tx, &loyer).await;
    }

    let (status, corps) = forecast(&db, ALICE, MOIS_PREVU).await;

    assert_eq!(status, StatusCode::OK, "{corps}");
    assert_eq!(entier(&corps, "revenus_restants_cents"), 0, "{corps}");
    assert_eq!(
        entier(&corps, "depenses_restantes_cents"),
        95_000,
        "{corps}"
    );
    assert_eq!(
        entier(&corps, "solde_previsionnel_cents"),
        -95_000,
        "{corps}"
    );

    db.destroy().await;
}

#[tokio::test]
async fn un_salaire_a_montant_et_libelle_variables_est_pris_en_compte() {
    // Cas réel : trois mois, trois libellés (le mois est collé au libellé) et
    // trois montants distants de plusieurs dizaines d'euros. La détection de
    // récurrence, calée sur des montants fixes, n'y voit rien ; la médiane des
    // crédits par catégorie, si.
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let salaire = categorie_id_par_nom(&db, ALICE, "Salaire").await;

    let avril = semer_transaction(
        &db,
        &compte,
        "VIREMENT BLUE SOFT VIREMENT-SALAIRE-AVRIL-26",
        121_788,
        jour(2026, 4, 28),
    )
    .await;
    let mai = semer_transaction(
        &db,
        &compte,
        "VIREMENT BLUE SOFT VIREMENT-SALAIRE-MAI-26",
        130_183,
        jour(2026, 5, 29),
    )
    .await;
    let juin = semer_transaction(
        &db,
        &compte,
        "VIREMENT BLUE SOFT VIREMENT-SALAIRE-JUIN-26",
        120_926,
        jour(2026, 6, 30),
    )
    .await;
    recalculer_recurrences(&db, ALICE).await;
    for tx in [avril, mai, juin] {
        categoriser(&db, ALICE, &compte, tx, &salaire).await;
    }

    let (status, corps) = forecast(&db, ALICE, MOIS_PREVU).await;

    assert_eq!(status, StatusCode::OK, "{corps}");
    assert_eq!(
        corps["donnees_suffisantes"],
        json!(true),
        "un salaire variable doit suffire à établir un prévisionnel : {corps}"
    );
    assert_eq!(
        entier(&corps, "revenus_restants_cents"),
        121_788,
        "médiane de 1217,88 / 1301,83 / 1209,26 : {corps}"
    );
    assert_eq!(entier(&corps, "depenses_restantes_cents"), 0, "{corps}");
    assert_eq!(
        entier(&corps, "solde_previsionnel_cents"),
        121_788,
        "{corps}"
    );

    db.destroy().await;
}

#[tokio::test]
async fn un_credit_range_dans_une_categorie_de_depense_n_est_pas_un_revenu() {
    // Un remboursement encaissé chaque mois et rangé dans « Courses » ne doit
    // pas gonfler les revenus prévisionnels : seules les catégories de revenu
    // comptent.
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let courses = categorie_id_par_nom(&db, ALICE, "Courses").await;
    let salaire = categorie_id_par_nom(&db, ALICE, "Salaire").await;

    for mois in [4u32, 5, 6] {
        let remboursement = semer_transaction(
            &db,
            &compte,
            "REMBOURSEMENT COURSES",
            5_000,
            jour(2026, mois, 10),
        )
        .await;
        categoriser(&db, ALICE, &compte, remboursement, &courses).await;
    }
    let revenus = semer_revenu_recurrent(&db, &compte, 200_000).await;
    recalculer_recurrences(&db, ALICE).await;
    for tx in revenus {
        categoriser(&db, ALICE, &compte, tx, &salaire).await;
    }

    let (status, corps) = forecast(&db, ALICE, MOIS_PREVU).await;

    assert_eq!(status, StatusCode::OK, "{corps}");
    assert_eq!(
        entier(&corps, "revenus_restants_cents"),
        200_000,
        "seul le salaire doit compter comme revenu : {corps}"
    );

    db.destroy().await;
}

#[tokio::test]
async fn ca02_detail_par_categorie_est_coherent_avec_les_totaux_de_tete() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    let salaire = categorie_id_par_nom(&db, ALICE, "Salaire").await;
    let loyer = categorie_id_par_nom(&db, ALICE, "Loyer").await;
    let courses = categorie_id_par_nom(&db, ALICE, "Courses").await;

    let revenus = semer_revenu_recurrent(&db, &compte, 200_000).await;
    let depenses = semer_depense_recurrente(&db, &compte, 80_000).await;
    recalculer_recurrences(&db, ALICE).await;
    for tx in revenus {
        categoriser(&db, ALICE, &compte, tx, &salaire).await;
    }
    for tx in depenses {
        categoriser(&db, ALICE, &compte, tx, &loyer).await;
    }
    definir_budget(&db, ALICE, &courses, 30_000, MOIS_PREVU).await;

    let (status, corps) = forecast(&db, ALICE, MOIS_PREVU).await;

    assert_eq!(status, StatusCode::OK, "{corps}");
    let categories = corps["categories"]
        .as_array()
        .unwrap_or_else(|| panic!("categories doit être un tableau : {corps}"));

    let ligne = |id: &str| -> Value {
        categories
            .iter()
            .find(|c| c["category_id"] == json!(id))
            .unwrap_or_else(|| panic!("catégorie {id} absente du détail : {corps}"))
            .clone()
    };

    let ligne_salaire = ligne(&salaire);
    assert_eq!(entier(&ligne_salaire, "restant_cents"), 200_000, "{corps}");

    let ligne_loyer = ligne(&loyer);
    assert_eq!(entier(&ligne_loyer, "restant_cents"), 80_000, "{corps}");

    let somme_restants: i64 = categories
        .iter()
        .map(|c| c["restant_cents"].as_i64().unwrap_or(0))
        .sum();

    assert_eq!(
        somme_restants,
        entier(&corps, "revenus_restants_cents") + entier(&corps, "depenses_restantes_cents"),
        "le détail par catégorie doit couvrir exactement les totaux de tête : {corps}"
    );

    db.destroy().await;
}

#[tokio::test]
async fn ca03_sans_recurrence_les_donnees_sont_insuffisantes() {
    let db = db_or_skip!();
    let compte = semer_compte(&db, ALICE).await;
    semer_transaction(&db, &compte, "ACHAT UNIQUE", -4_990, jour(2026, 6, 10)).await;
    recalculer_recurrences(&db, ALICE).await;

    let (status, corps) = forecast(&db, ALICE, MOIS_PREVU).await;

    assert_eq!(status, StatusCode::OK, "{corps}");
    assert_eq!(
        corps["donnees_suffisantes"],
        json!(false),
        "sans récurrence détectée, donnees_suffisantes doit être false : {corps}"
    );

    db.destroy().await;
}

#[tokio::test]
async fn ca03_owner_sans_aucune_transaction_a_des_donnees_insuffisantes() {
    let db = db_or_skip!();
    semer_compte(&db, ALICE).await;

    let (status, corps) = forecast(&db, ALICE, MOIS_PREVU).await;

    assert_eq!(status, StatusCode::OK, "{corps}");
    assert_eq!(corps["donnees_suffisantes"], json!(false), "{corps}");

    db.destroy().await;
}

#[tokio::test]
async fn isolation_le_previsionnel_d_un_owner_ignore_les_donnees_d_un_autre() {
    let db = db_or_skip!();
    let compte_alice = semer_compte(&db, ALICE).await;
    let salaire_alice = categorie_id_par_nom(&db, ALICE, "Salaire").await;
    let courses_alice = categorie_id_par_nom(&db, ALICE, "Courses").await;

    let revenus = semer_revenu_recurrent(&db, &compte_alice, 250_000).await;
    recalculer_recurrences(&db, ALICE).await;
    for tx in revenus {
        categoriser(&db, ALICE, &compte_alice, tx, &salaire_alice).await;
    }
    definir_budget(&db, ALICE, &courses_alice, 30_000, MOIS_PREVU).await;

    let (status_bob, corps_bob) = forecast(&db, BOB, MOIS_PREVU).await;
    assert_eq!(status_bob, StatusCode::OK, "{corps_bob}");
    assert_eq!(
        entier(&corps_bob, "revenus_restants_cents"),
        0,
        "{corps_bob}"
    );
    assert_eq!(
        entier(&corps_bob, "depenses_restantes_cents"),
        0,
        "{corps_bob}"
    );
    assert_eq!(
        corps_bob["donnees_suffisantes"],
        json!(false),
        "Bob n'a aucune récurrence : {corps_bob}"
    );

    let (status_alice, corps_alice) = forecast(&db, ALICE, MOIS_PREVU).await;
    assert_eq!(status_alice, StatusCode::OK, "{corps_alice}");
    assert_eq!(
        entier(&corps_alice, "revenus_restants_cents"),
        250_000,
        "{corps_alice}"
    );

    db.destroy().await;
}

#[tokio::test]
async fn mois_malforme_est_rejete_par_une_erreur_client() {
    let db = db_or_skip!();
    semer_compte(&db, ALICE).await;

    let (status, corps) = forecast(&db, ALICE, "2026-13").await;

    assert!(
        status.is_client_error(),
        "un mois malformé doit être rejeté par une erreur 4xx, reçu {status} : {corps}"
    );

    db.destroy().await;
}

#[tokio::test]
async fn mois_absent_est_rejete_par_une_erreur_client() {
    let db = db_or_skip!();
    semer_compte(&db, ALICE).await;

    let (status, corps) = appel(&db, "GET", "/v1/forecast", ALICE, None).await;

    assert!(
        status.is_client_error(),
        "un paramètre month absent doit être rejeté par une erreur 4xx, reçu {status} : {corps}"
    );

    db.destroy().await;
}
