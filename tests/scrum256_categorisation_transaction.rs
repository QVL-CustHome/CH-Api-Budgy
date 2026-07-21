mod common;

use std::sync::Arc;

use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::domain::bank_account::{BankAccountId, NouveauBankAccount};
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::{ConsentId, ConsentStatus, NouveauConsent};
use ch_api_budgy::domain::ports::ecriture::{
    BankAccountsWriteRepository, BankTransactionsWriteRepository, ConsentsWriteRepository,
    ResultatInsertion,
};
use ch_api_budgy::domain::transaction_bancaire::{
    CategorizationSource, NouvelleTransactionBancaire, TransactionStatus,
};
use ch_api_budgy::repository::bank_accounts::SqlxBankAccountsWriteAdapter;
use ch_api_budgy::repository::bank_transactions::SqlxBankTransactionsWriteAdapter;
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
use chrono::{TimeZone, Utc};
use common::DisposableDb;
use uuid::Uuid;

macro_rules! db_or_skip {
    () => {
        match DisposableDb::create().await {
            Some(db) => {
                db.migrate().await;
                db
            }
            None => {
                eprintln!(
                    "SCRUM-256 ignoré : variable {} absente (Postgres jetable requis)",
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

async fn semer_transaction(db: &DisposableDb, crypto: &Arc<CryptoService>, owner: &str) -> Uuid {
    let proprietaire = ProprietaireId(owner.to_string());
    let consents = SqlxConsentsWriteAdapter::new(db.pool.clone(), crypto.clone());
    let comptes = SqlxBankAccountsWriteAdapter::new(db.pool.clone(), crypto.clone());
    let transactions = SqlxBankTransactionsWriteAdapter::new(db.pool.clone(), crypto.clone());

    let consent_id = ConsentsWriteRepository::enregistrer(
        &consents,
        NouveauConsent {
            proprietaire: proprietaire.clone(),
            external_ref: format!("ref-{owner}"),
            status: ConsentStatus::Active,
            expires_at: Some(Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap()),
        },
    )
    .await
    .expect("consent semé");

    let account_id = enregistrer_compte(&comptes, &proprietaire, consent_id, owner).await;

    let inseree = BankTransactionsWriteRepository::enregistrer(
        &transactions,
        NouvelleTransactionBancaire {
            bank_account: account_id,
            external_transaction_id: format!("tx-{owner}"),
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
        ResultatInsertion::Doublon => panic!("la transaction devait être insérée, pas dédupliquée"),
    }
}

async fn enregistrer_compte(
    comptes: &SqlxBankAccountsWriteAdapter,
    proprietaire: &ProprietaireId,
    consent_id: ConsentId,
    owner: &str,
) -> BankAccountId {
    BankAccountsWriteRepository::enregistrer(
        comptes,
        NouveauBankAccount {
            proprietaire: proprietaire.clone(),
            consent: consent_id,
            external_account_id: format!("acct-{owner}"),
            iban: "FR7630006000011234567890189".to_string(),
            currency: "EUR".to_string(),
            next_sync_at: None,
        },
    )
    .await
    .expect("compte semé")
}

async fn premiere_categorie(db: &DisposableDb) -> Uuid {
    sqlx::query_scalar("SELECT id FROM budgy.category ORDER BY kind, name LIMIT 1")
        .fetch_one(&db.pool)
        .await
        .expect("au moins une catégorie doit exister via le seed 0009")
}

async fn categories_distinctes(db: &DisposableDb) -> (Uuid, Uuid) {
    let ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM budgy.category ORDER BY kind, name LIMIT 2")
            .fetch_all(&db.pool)
            .await
            .expect("deux catégories requises");
    (ids[0], ids[1])
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
    .expect("lecture de la catégorisation de la transaction")
}

async fn rattacher_manuellement(db: &DisposableDb, tx_id: Uuid, categorie: Uuid, regle: Uuid) {
    sqlx::query(
        "UPDATE budgy.bank_transaction \
         SET category_id = $2, categorization_source = 'manual', rule_id = $3 WHERE id = $1",
    )
    .bind(tx_id)
    .bind(categorie)
    .bind(regle)
    .execute(&db.pool)
    .await
    .expect("rattachement manuel de la transaction");
}

#[tokio::test]
async fn migration_0010_s_applique_sans_erreur() {
    let db = db_or_skip!();

    let succes: bool =
        sqlx::query_scalar("SELECT success FROM _sqlx_migrations WHERE version = 10")
            .fetch_one(&db.pool)
            .await
            .expect("la migration 0010 doit être tracée");

    assert!(succes, "la migration 0010 doit être marquée appliquée");

    db.destroy().await;
}

#[tokio::test]
async fn colonne_category_id_nullable_de_type_uuid() {
    let db = db_or_skip!();

    let (is_nullable, data_type): (String, String) = sqlx::query_as(
        "SELECT is_nullable, data_type FROM information_schema.columns \
         WHERE table_schema = 'budgy' AND table_name = 'bank_transaction' \
         AND column_name = 'category_id'",
    )
    .fetch_one(&db.pool)
    .await
    .expect("la colonne category_id doit exister");

    assert_eq!(is_nullable, "YES", "category_id doit être nullable");
    assert_eq!(data_type, "uuid", "category_id doit être un UUID");

    db.destroy().await;
}

#[tokio::test]
async fn colonne_rule_id_nullable_de_type_uuid() {
    let db = db_or_skip!();

    let (is_nullable, data_type): (String, String) = sqlx::query_as(
        "SELECT is_nullable, data_type FROM information_schema.columns \
         WHERE table_schema = 'budgy' AND table_name = 'bank_transaction' \
         AND column_name = 'rule_id'",
    )
    .fetch_one(&db.pool)
    .await
    .expect("la colonne rule_id doit exister");

    assert_eq!(is_nullable, "YES", "rule_id doit être nullable");
    assert_eq!(data_type, "uuid", "rule_id doit être un UUID");

    db.destroy().await;
}

#[tokio::test]
async fn colonne_categorization_source_par_defaut_non_categorisee() {
    let db = db_or_skip!();

    let (is_nullable, column_default): (String, Option<String>) = sqlx::query_as(
        "SELECT is_nullable, column_default FROM information_schema.columns \
         WHERE table_schema = 'budgy' AND table_name = 'bank_transaction' \
         AND column_name = 'categorization_source'",
    )
    .fetch_one(&db.pool)
    .await
    .expect("la colonne categorization_source doit exister");

    assert_eq!(
        is_nullable, "NO",
        "categorization_source ne doit pas être nullable"
    );
    assert_eq!(
        column_default.as_deref(),
        Some("'none'::text"),
        "la source par défaut doit être 'none' (non catégorisée)"
    );

    db.destroy().await;
}

#[tokio::test]
async fn category_id_est_une_cle_etrangere_vers_category() {
    let db = db_or_skip!();

    let table_referencee: String = sqlx::query_scalar(
        "SELECT ccu.table_name \
         FROM information_schema.table_constraints tc \
         JOIN information_schema.key_column_usage kcu \
           ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema \
         JOIN information_schema.constraint_column_usage ccu \
           ON tc.constraint_name = ccu.constraint_name AND tc.table_schema = ccu.table_schema \
         WHERE tc.constraint_type = 'FOREIGN KEY' \
           AND tc.table_schema = 'budgy' AND tc.table_name = 'bank_transaction' \
           AND kcu.column_name = 'category_id'",
    )
    .fetch_one(&db.pool)
    .await
    .expect("category_id doit porter une clé étrangère");

    assert_eq!(
        table_referencee, "category",
        "category_id doit référencer budgy.category"
    );

    db.destroy().await;
}

#[tokio::test]
async fn transaction_importee_est_non_categorisee_par_defaut() {
    let db = db_or_skip!();
    let crypto = crypto();

    let tx_id = semer_transaction(&db, &crypto, "owner-256-import").await;

    let (category_id, source, rule_id) = lire_categorisation(&db, tx_id).await;

    assert_eq!(
        category_id, None,
        "une transaction importée ne doit pas être catégorisée"
    );
    assert_eq!(
        source, "none",
        "la source doit être 'none' pour une transaction non catégorisée"
    );
    assert_eq!(
        rule_id, None,
        "aucune règle ne doit être renseignée à l'import"
    );

    db.destroy().await;
}

#[tokio::test]
async fn categorization_source_accepte_les_trois_valeurs_de_l_enum() {
    let db = db_or_skip!();
    let crypto = crypto();
    let tx_id = semer_transaction(&db, &crypto, "owner-256-enum-ok").await;

    for valeur in ["manual", "rule", "none"] {
        let resultat = sqlx::query(
            "UPDATE budgy.bank_transaction SET categorization_source = $2 WHERE id = $1",
        )
        .bind(tx_id)
        .bind(valeur)
        .execute(&db.pool)
        .await;
        assert!(resultat.is_ok(), "la valeur '{valeur}' doit être acceptée");
    }

    db.destroy().await;
}

#[test]
fn categorization_source_rejette_une_valeur_hors_enum() {
    assert_eq!(
        CategorizationSource::parse("manual"),
        Some(CategorizationSource::Manual)
    );
    assert_eq!(
        CategorizationSource::parse("rule"),
        Some(CategorizationSource::Rule)
    );
    assert_eq!(
        CategorizationSource::parse("none"),
        Some(CategorizationSource::None)
    );
    assert_eq!(CategorizationSource::parse("valeur_hors_enum"), None);
}

#[tokio::test]
async fn transaction_peut_etre_rattachee_a_une_categorie() {
    let db = db_or_skip!();
    let crypto = crypto();
    let tx_id = semer_transaction(&db, &crypto, "owner-256-rattache").await;
    let categorie = premiere_categorie(&db).await;
    let regle = Uuid::new_v4();

    rattacher_manuellement(&db, tx_id, categorie, regle).await;

    let (category_id, source, rule_id) = lire_categorisation(&db, tx_id).await;
    assert_eq!(
        category_id,
        Some(categorie),
        "la transaction doit pointer vers la catégorie"
    );
    assert_eq!(
        source, "manual",
        "la source doit refléter la catégorisation manuelle"
    );
    assert_eq!(
        rule_id,
        Some(regle),
        "la règle appliquée doit être renseignée"
    );

    db.destroy().await;
}

#[tokio::test]
async fn suppression_categorie_repasse_transaction_non_categorisee() {
    let db = db_or_skip!();
    let crypto = crypto();
    let tx_id = semer_transaction(&db, &crypto, "owner-256-suppr").await;
    let categorie = premiere_categorie(&db).await;
    rattacher_manuellement(&db, tx_id, categorie, Uuid::new_v4()).await;

    let suppression = sqlx::query("DELETE FROM budgy.category WHERE id = $1")
        .bind(categorie)
        .execute(&db.pool)
        .await;
    assert!(
        suppression.is_ok(),
        "la suppression d'une catégorie utilisée ne doit pas casser les transactions"
    );

    let (category_id, source, rule_id) = lire_categorisation(&db, tx_id).await;
    assert_eq!(
        category_id, None,
        "la transaction ne doit plus pointer vers la catégorie supprimée"
    );
    assert_eq!(source, "none", "la source doit être réinitialisée à 'none'");
    assert_eq!(
        rule_id, None,
        "la règle doit être effacée quand la catégorie disparaît"
    );

    db.destroy().await;
}

#[tokio::test]
async fn suppression_categorie_n_affecte_que_les_transactions_concernees() {
    let db = db_or_skip!();
    let crypto = crypto();
    let tx_concernee = semer_transaction(&db, &crypto, "owner-256-iso-a").await;
    let tx_intacte = semer_transaction(&db, &crypto, "owner-256-iso-b").await;
    let (cat_supprimee, cat_conservee) = categories_distinctes(&db).await;
    let regle_intacte = Uuid::new_v4();

    rattacher_manuellement(&db, tx_concernee, cat_supprimee, Uuid::new_v4()).await;
    rattacher_manuellement(&db, tx_intacte, cat_conservee, regle_intacte).await;

    sqlx::query("DELETE FROM budgy.category WHERE id = $1")
        .bind(cat_supprimee)
        .execute(&db.pool)
        .await
        .expect("suppression de la catégorie ciblée");

    let (cat_a, source_a, rule_a) = lire_categorisation(&db, tx_concernee).await;
    assert_eq!(cat_a, None);
    assert_eq!(source_a, "none");
    assert_eq!(rule_a, None);

    let (cat_b, source_b, rule_b) = lire_categorisation(&db, tx_intacte).await;
    assert_eq!(
        cat_b,
        Some(cat_conservee),
        "l'autre transaction doit rester catégorisée"
    );
    assert_eq!(source_b, "manual");
    assert_eq!(rule_b, Some(regle_intacte));

    db.destroy().await;
}
