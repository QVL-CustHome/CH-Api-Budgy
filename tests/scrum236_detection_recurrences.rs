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
use ch_api_budgy::domain::transaction_bancaire::{NouvelleTransactionBancaire, TransactionStatus};
use ch_api_budgy::repository::bank_accounts::SqlxBankAccountsWriteAdapter;
use ch_api_budgy::repository::bank_transactions::SqlxBankTransactionsWriteAdapter;
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
use chrono::{NaiveDate, TimeZone, Utc};
use common::DisposableDb;
use uuid::Uuid;

macro_rules! fixture_or_skip {
    ($owner:expr) => {
        match Fixture::creer($owner).await {
            Some(fixture) => fixture,
            None => {
                eprintln!(
                    "SCRUM-236 ignoré : variable {} absente (Postgres jetable requis)",
                    common::ENV_ADMIN_URL
                );
                return;
            }
        }
    };
}

struct Fixture {
    db: DisposableDb,
    transactions: SqlxBankTransactionsWriteAdapter,
    proprietaire: ProprietaireId,
    compte: BankAccountId,
}

impl Fixture {
    async fn creer(owner: &str) -> Option<Self> {
        let db = DisposableDb::create().await?;
        db.migrate().await;

        let crypto = Arc::new(CryptoService::from_key(&[7u8; 32]).expect("clé de test 32 octets"));
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

        let compte = Self::enregistrer_compte(&comptes, &proprietaire, consent_id, owner).await;

        Some(Self {
            db,
            transactions,
            proprietaire,
            compte,
        })
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

    async fn inserer(
        &self,
        reference: &str,
        label: &str,
        amount_cents: i64,
        jour: NaiveDate,
    ) -> Uuid {
        let inseree = BankTransactionsWriteRepository::enregistrer(
            &self.transactions,
            NouvelleTransactionBancaire {
                bank_account: self.compte.clone(),
                external_transaction_id: format!("tx-{reference}"),
                status: TransactionStatus::Booked,
                label: label.to_string(),
                amount_cents,
                currency: "EUR".to_string(),
                booking_date: Some(jour),
                value_date: Some(jour),
            },
        )
        .await
        .expect("transaction semée");

        match inseree {
            ResultatInsertion::Inseree(id) => id.0,
            ResultatInsertion::Doublon => panic!("transaction dédupliquée à tort"),
        }
    }

    async fn recalculer(&self) -> u64 {
        self.transactions
            .recalculer_recurrences(&self.proprietaire)
            .await
            .expect("recalcul des récurrences")
    }

    async fn recurrence(&self, tx_id: Uuid) -> (bool, Option<String>) {
        sqlx::query_as(
            "SELECT is_recurrent, recurrence_interval FROM budgy.bank_transaction WHERE id = $1",
        )
        .bind(tx_id)
        .fetch_one(&self.db.pool)
        .await
        .expect("lecture de la récurrence de la transaction")
    }

    async fn detruire(self) {
        self.db.destroy().await;
    }
}

fn jour(annee: i32, mois: u32, jour: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(annee, mois, jour).expect("date valide")
}

#[tokio::test]
async fn ca01_trois_occurrences_mensuelles_meme_label_montant_proche_sont_recurrentes() {
    let fixture = fixture_or_skip!("owner-236-ca01-nominal");

    let janvier = fixture
        .inserer("nom-jan", "NETFLIX ABONNEMENT", -1_299, jour(2026, 1, 15))
        .await;
    let fevrier = fixture
        .inserer("nom-fev", "NETFLIX ABONNEMENT", -1_320, jour(2026, 2, 15))
        .await;
    let mars = fixture
        .inserer("nom-mar", "NETFLIX ABONNEMENT", -1_280, jour(2026, 3, 15))
        .await;

    let marquees = fixture.recalculer().await;
    assert_eq!(marquees, 3, "les trois occurrences doivent être marquées");

    for tx in [janvier, fevrier, mars] {
        let (recurrente, intervalle) = fixture.recurrence(tx).await;
        assert!(recurrente, "chaque occurrence doit être marquée récurrente");
        assert_eq!(
            intervalle.as_deref(),
            Some("monthly"),
            "la périodicité identifiée doit être mensuelle"
        );
    }

    fixture.detruire().await;
}

#[tokio::test]
async fn ca01_intervalles_aux_bornes_26_et_35_jours_restent_mensuels() {
    let fixture = fixture_or_skip!("owner-236-ca01-bornes");

    let premiere = fixture
        .inserer("b1", "LOYER GARAGE", -5_000, jour(2026, 1, 1))
        .await;
    let deuxieme = fixture
        .inserer("b2", "LOYER GARAGE", -5_000, jour(2026, 1, 27))
        .await;
    let troisieme = fixture
        .inserer("b3", "LOYER GARAGE", -5_000, jour(2026, 3, 3))
        .await;

    fixture.recalculer().await;

    for tx in [premiere, deuxieme, troisieme] {
        let (recurrente, intervalle) = fixture.recurrence(tx).await;
        assert!(
            recurrente,
            "un intervalle de 26 puis 35 jours reste mensuel"
        );
        assert_eq!(intervalle.as_deref(), Some("monthly"));
    }

    fixture.detruire().await;
}

#[tokio::test]
async fn ca02_transaction_ponctuelle_n_est_pas_recurrente() {
    let fixture = fixture_or_skip!("owner-236-ca02-ponctuelle");

    let ponctuelle = fixture
        .inserer("uniq", "ACHAT MEUBLE", -49_900, jour(2026, 1, 10))
        .await;

    let marquees = fixture.recalculer().await;
    assert_eq!(marquees, 0, "aucune transaction ne doit être marquée");

    let (recurrente, intervalle) = fixture.recurrence(ponctuelle).await;
    assert!(!recurrente, "une transaction unique n'est pas récurrente");
    assert_eq!(intervalle, None, "aucune périodicité ne doit être posée");

    fixture.detruire().await;
}

#[tokio::test]
async fn ca02_deux_occurrences_mensuelles_sous_le_seuil_ne_sont_pas_recurrentes() {
    let fixture = fixture_or_skip!("owner-236-ca02-seuil");

    let janvier = fixture
        .inserer("s1", "SPOTIFY PREMIUM", -1_099, jour(2026, 1, 5))
        .await;
    let fevrier = fixture
        .inserer("s2", "SPOTIFY PREMIUM", -1_099, jour(2026, 2, 5))
        .await;

    let marquees = fixture.recalculer().await;
    assert_eq!(marquees, 0, "deux occurrences restent sous le seuil");

    for tx in [janvier, fevrier] {
        let (recurrente, intervalle) = fixture.recurrence(tx).await;
        assert!(!recurrente, "sous le seuil de récurrence, pas de marquage");
        assert_eq!(intervalle, None);
    }

    fixture.detruire().await;
}

#[tokio::test]
async fn ca02_montants_hors_tolerance_ne_sont_pas_recurrents() {
    let fixture = fixture_or_skip!("owner-236-ca02-montant");

    let janvier = fixture
        .inserer("m1", "COURSES SUPERMARCHE", -1_000, jour(2026, 1, 12))
        .await;
    let fevrier = fixture
        .inserer("m2", "COURSES SUPERMARCHE", -2_000, jour(2026, 2, 12))
        .await;
    let mars = fixture
        .inserer("m3", "COURSES SUPERMARCHE", -3_000, jour(2026, 3, 12))
        .await;

    let marquees = fixture.recalculer().await;
    assert_eq!(
        marquees, 0,
        "des montants trop éloignés ne forment pas une récurrence"
    );

    for tx in [janvier, fevrier, mars] {
        let (recurrente, _) = fixture.recurrence(tx).await;
        assert!(!recurrente, "montant hors tolérance : pas de récurrence");
    }

    fixture.detruire().await;
}

#[tokio::test]
async fn ca02_intervalle_hebdomadaire_hors_fenetre_mensuelle_n_est_pas_recurrent() {
    let fixture = fixture_or_skip!("owner-236-ca02-intervalle");

    let semaine1 = fixture
        .inserer("h1", "PLEIN ESSENCE", -6_000, jour(2026, 1, 1))
        .await;
    let semaine2 = fixture
        .inserer("h2", "PLEIN ESSENCE", -6_000, jour(2026, 1, 8))
        .await;
    let semaine3 = fixture
        .inserer("h3", "PLEIN ESSENCE", -6_000, jour(2026, 1, 15))
        .await;

    let marquees = fixture.recalculer().await;
    assert_eq!(marquees, 0, "un rythme hebdomadaire n'est pas mensuel");

    for tx in [semaine1, semaine2, semaine3] {
        let (recurrente, _) = fixture.recurrence(tx).await;
        assert!(
            !recurrente,
            "intervalle hors [26,35] jours : pas de récurrence"
        );
    }

    fixture.detruire().await;
}

#[tokio::test]
async fn ca02_labels_differents_ne_sont_pas_regroupes_en_recurrence() {
    let fixture = fixture_or_skip!("owner-236-ca02-labels");

    let janvier = fixture
        .inserer("l1", "MARCHAND ALPHA", -2_500, jour(2026, 1, 20))
        .await;
    let fevrier = fixture
        .inserer("l2", "MARCHAND BETA", -2_500, jour(2026, 2, 20))
        .await;
    let mars = fixture
        .inserer("l3", "MARCHAND GAMMA", -2_500, jour(2026, 3, 20))
        .await;

    let marquees = fixture.recalculer().await;
    assert_eq!(
        marquees, 0,
        "des marchands différents ne forment pas une récurrence"
    );

    for tx in [janvier, fevrier, mars] {
        let (recurrente, _) = fixture.recurrence(tx).await;
        assert!(!recurrente, "labels distincts : pas de récurrence");
    }

    fixture.detruire().await;
}
