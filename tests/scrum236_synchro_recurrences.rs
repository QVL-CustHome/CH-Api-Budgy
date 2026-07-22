mod common;

use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::domain::balance::{Balance, BalanceId, BalanceType};
use ch_api_budgy::domain::bank_account::{BankAccount, BankAccountId, NouveauBankAccount};
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::{Consent, ConsentStatus, NouveauConsent};
use ch_api_budgy::domain::horloge::Horloge;
use ch_api_budgy::domain::ports::bank_data_source::{
    BankDataSource, BankDataSourceError, ConsentementInitie, DemandeConsentement, Etablissement,
    ReponseAutorisation,
};
use ch_api_budgy::domain::ports::ecriture::{
    BankAccountsWriteRepository, BankTransactionsWriteRepository, ConsentsWriteRepository,
    ResultatInsertion,
};
use ch_api_budgy::domain::ports::evenement_synchro::NoopEventPublisher;
use ch_api_budgy::domain::synchro::{DependancesSynchro, ParametresSynchro, SynchroComptes};
use ch_api_budgy::domain::transaction_bancaire::{
    CategorizationSource, NouvelleTransactionBancaire, TransactionBancaire, TransactionBancaireId,
    TransactionStatus,
};
use ch_api_budgy::repository::balances::SqlxBalancesWriteAdapter;
use ch_api_budgy::repository::bank_accounts::SqlxBankAccountsWriteAdapter;
use ch_api_budgy::repository::bank_transactions::SqlxBankTransactionsWriteAdapter;
use ch_api_budgy::repository::consents::SqlxConsentsWriteAdapter;
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use common::DisposableDb;
use uuid::Uuid;

macro_rules! banc_or_skip {
    () => {
        match Banc::creer().await {
            Some(banc) => banc,
            None => {
                eprintln!(
                    "SCRUM-236 wiring synchro ignoré : variable {} absente (Postgres jetable requis)",
                    common::ENV_ADMIN_URL
                );
                return;
            }
        }
    };
}

struct HorlogeFixe(DateTime<Utc>);

impl Horloge for HorlogeFixe {
    fn maintenant(&self) -> DateTime<Utc> {
        self.0
    }
}

struct SourceProgrammable {
    lot: Mutex<Vec<TransactionBancaire>>,
}

impl SourceProgrammable {
    fn avec(transactions: Vec<TransactionBancaire>) -> Arc<Self> {
        Arc::new(Self {
            lot: Mutex::new(transactions),
        })
    }

    fn vide() -> Arc<Self> {
        Arc::new(Self {
            lot: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl BankDataSource for SourceProgrammable {
    async fn lister_etablissements(&self) -> Result<Vec<Etablissement>, BankDataSourceError> {
        Ok(Vec::new())
    }

    async fn initier_consentement(
        &self,
        _demande: DemandeConsentement,
    ) -> Result<ConsentementInitie, BankDataSourceError> {
        Err(BankDataSourceError::SourceNonConfiguree)
    }

    async fn completer_consentement(
        &self,
        _proprietaire: &ProprietaireId,
        _reponse: ReponseAutorisation,
    ) -> Result<Consent, BankDataSourceError> {
        Err(BankDataSourceError::SourceNonConfiguree)
    }

    async fn lister_comptes(
        &self,
        _consent: &Consent,
    ) -> Result<Vec<BankAccount>, BankDataSourceError> {
        Ok(Vec::new())
    }

    async fn solde(
        &self,
        _consent: &Consent,
        compte: &BankAccount,
    ) -> Result<Vec<Balance>, BankDataSourceError> {
        Ok(vec![Balance {
            id: BalanceId(Uuid::new_v4()),
            bank_account: compte.id.clone(),
            balance_type: BalanceType::Available,
            amount_cents: 100_000,
            currency: compte.currency.clone(),
            reference_date: Utc.with_ymd_and_hms(2026, 6, 25, 0, 0, 0).unwrap(),
            created_at: Utc.with_ymd_and_hms(2026, 6, 25, 0, 0, 0).unwrap(),
        }])
    }

    async fn lister_transactions(
        &self,
        _consent: &Consent,
        _compte: &BankAccount,
        _depuis: NaiveDate,
    ) -> Result<Vec<TransactionBancaire>, BankDataSourceError> {
        Ok(std::mem::take(&mut self.lot.lock().expect("lot programmé")))
    }

    async fn revoquer_consentement(
        &self,
        consent: &Consent,
    ) -> Result<Consent, BankDataSourceError> {
        Ok(consent.clone())
    }
}

type ServiceSynchro = SynchroComptes<
    SqlxBankAccountsWriteAdapter,
    SqlxBankAccountsWriteAdapter,
    SourceProgrammable,
    SqlxBalancesWriteAdapter,
    SqlxBankTransactionsWriteAdapter,
    SqlxConsentsWriteAdapter,
    HorlogeFixe,
    NoopEventPublisher,
>;

struct Banc {
    db: DisposableDb,
    crypto: Arc<CryptoService>,
}

impl Banc {
    async fn creer() -> Option<Self> {
        let db = DisposableDb::create().await?;
        db.migrate().await;
        let crypto = Arc::new(CryptoService::from_key(&[7u8; 32]).expect("clé de test 32 octets"));
        Some(Self { db, crypto })
    }

    async fn semer_compte(
        &self,
        owner: &str,
        next_sync_at: Option<DateTime<Utc>>,
    ) -> BankAccountId {
        let consents = SqlxConsentsWriteAdapter::new(self.db.pool.clone(), self.crypto.clone());
        let consent_id = ConsentsWriteRepository::enregistrer(
            &consents,
            NouveauConsent {
                proprietaire: ProprietaireId(owner.to_string()),
                external_ref: format!("ref-{owner}"),
                status: ConsentStatus::Active,
                expires_at: Some(Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap()),
            },
        )
        .await
        .expect("consent semé");

        let comptes = SqlxBankAccountsWriteAdapter::new(self.db.pool.clone(), self.crypto.clone());
        BankAccountsWriteRepository::enregistrer(
            &comptes,
            NouveauBankAccount {
                proprietaire: ProprietaireId(owner.to_string()),
                consent: consent_id,
                external_account_id: format!("acct-{owner}"),
                iban: "FR7630006000011234567890189".to_string(),
                currency: "EUR".to_string(),
                next_sync_at,
            },
        )
        .await
        .expect("compte semé")
    }

    async fn inserer(
        &self,
        compte: &BankAccountId,
        reference: &str,
        label: &str,
        amount_cents: i64,
        jour: NaiveDate,
    ) {
        let transactions =
            SqlxBankTransactionsWriteAdapter::new(self.db.pool.clone(), self.crypto.clone());
        let inseree = BankTransactionsWriteRepository::enregistrer(
            &transactions,
            NouvelleTransactionBancaire {
                bank_account: compte.clone(),
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
        assert!(
            matches!(inseree, ResultatInsertion::Inseree(_)),
            "la transaction de préparation doit être insérée"
        );
    }

    fn service(&self, source: Arc<SourceProgrammable>) -> ServiceSynchro {
        let dependances = DependancesSynchro {
            planification_lecture: SqlxBankAccountsWriteAdapter::new(
                self.db.pool.clone(),
                self.crypto.clone(),
            ),
            planification_ecriture: SqlxBankAccountsWriteAdapter::new(
                self.db.pool.clone(),
                self.crypto.clone(),
            ),
            source_bancaire: source,
            soldes: SqlxBalancesWriteAdapter::new(self.db.pool.clone(), self.crypto.clone()),
            transactions: SqlxBankTransactionsWriteAdapter::new(
                self.db.pool.clone(),
                self.crypto.clone(),
            ),
            consents_statut: SqlxConsentsWriteAdapter::new(
                self.db.pool.clone(),
                self.crypto.clone(),
            ),
            horloge: HorlogeFixe(maintenant()),
            publisher: Arc::new(NoopEventPublisher),
        };
        SynchroComptes::new(dependances, ParametresSynchro::default())
    }

    async fn recurrences(&self, compte: &BankAccountId) -> Vec<(bool, Option<String>)> {
        sqlx::query_as(
            "SELECT is_recurrent, recurrence_interval FROM budgy.bank_transaction \
             WHERE bank_account_id = $1 ORDER BY booking_date",
        )
        .bind(compte.0)
        .fetch_all(&self.db.pool)
        .await
        .expect("lecture des récurrences du compte")
    }

    async fn detruire(self) {
        self.db.destroy().await;
    }
}

fn maintenant() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 25, 9, 0, 0).unwrap()
}

fn jour(annee: i32, mois: u32, jour: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(annee, mois, jour).expect("date valide")
}

fn occurrence(reference: &str, amount_cents: i64, jour: NaiveDate) -> TransactionBancaire {
    TransactionBancaire {
        id: TransactionBancaireId(Uuid::new_v4()),
        bank_account: BankAccountId(Uuid::new_v4()),
        external_transaction_id: format!("tx-{reference}"),
        status: TransactionStatus::Booked,
        label: "NETFLIX ABONNEMENT".to_string(),
        amount_cents,
        currency: "EUR".to_string(),
        booking_date: Some(jour),
        value_date: Some(jour),
        category: None,
        categorization_source: CategorizationSource::None,
        rule_id: None,
        is_recurrent: false,
        recurrence_interval: None,
        created_at: maintenant(),
    }
}

fn trois_occurrences_mensuelles() -> Vec<TransactionBancaire> {
    vec![
        occurrence("jan", -1_299, jour(2026, 1, 15)),
        occurrence("fev", -1_320, jour(2026, 2, 15)),
        occurrence("mar", -1_280, jour(2026, 3, 15)),
    ]
}

#[tokio::test]
async fn un_cycle_de_synchro_marque_les_occurrences_mensuelles_inserees() {
    let banc = banc_or_skip!();
    let compte = banc
        .semer_compte("owner-236-wiring", Some(maintenant() - Duration::hours(1)))
        .await;
    let source = SourceProgrammable::avec(trois_occurrences_mensuelles());

    let rapport = banc
        .service(source)
        .executer()
        .await
        .expect("cycle synchro");
    assert_eq!(rapport.comptes_synchronises, 1);
    assert_eq!(rapport.transactions_inserees, 3);

    let recurrences = banc.recurrences(&compte).await;
    assert_eq!(
        recurrences.len(),
        3,
        "les trois occurrences sont persistées"
    );
    for (recurrente, intervalle) in recurrences {
        assert!(
            recurrente,
            "chaque occurrence insérée doit ressortir récurrente"
        );
        assert_eq!(
            intervalle.as_deref(),
            Some("monthly"),
            "la périodicité identifiée doit être mensuelle"
        );
    }

    banc.detruire().await;
}

#[tokio::test]
async fn le_recalcul_ne_deborde_pas_sur_un_autre_proprietaire() {
    let banc = banc_or_skip!();
    let synchronise = banc
        .semer_compte("owner-236-synchro", Some(maintenant() - Duration::hours(1)))
        .await;
    let voisin = banc
        .semer_compte("owner-236-voisin", Some(maintenant() + Duration::hours(3)))
        .await;

    banc.inserer(
        &voisin,
        "v-jan",
        "NETFLIX ABONNEMENT",
        -1_299,
        jour(2026, 1, 15),
    )
    .await;
    banc.inserer(
        &voisin,
        "v-fev",
        "NETFLIX ABONNEMENT",
        -1_320,
        jour(2026, 2, 15),
    )
    .await;
    banc.inserer(
        &voisin,
        "v-mar",
        "NETFLIX ABONNEMENT",
        -1_280,
        jour(2026, 3, 15),
    )
    .await;

    let source = SourceProgrammable::avec(trois_occurrences_mensuelles());
    let rapport = banc
        .service(source)
        .executer()
        .await
        .expect("cycle synchro");
    assert_eq!(
        rapport.comptes_synchronises, 1,
        "seul le compte échéant est synchronisé"
    );

    for (recurrente, _) in banc.recurrences(&synchronise).await {
        assert!(
            recurrente,
            "le propriétaire synchronisé voit ses récurrences marquées"
        );
    }
    for (recurrente, intervalle) in banc.recurrences(&voisin).await {
        assert!(!recurrente, "le voisin non synchronisé n'est jamais marqué");
        assert_eq!(intervalle, None, "aucune périodicité posée chez le voisin");
    }

    banc.detruire().await;
}

#[tokio::test]
async fn sans_insertion_le_recalcul_n_est_pas_declenche() {
    let banc = banc_or_skip!();
    let compte = banc
        .semer_compte(
            "owner-236-sans-insert",
            Some(maintenant() - Duration::hours(1)),
        )
        .await;

    banc.inserer(
        &compte,
        "p-jan",
        "NETFLIX ABONNEMENT",
        -1_299,
        jour(2026, 1, 15),
    )
    .await;
    banc.inserer(
        &compte,
        "p-fev",
        "NETFLIX ABONNEMENT",
        -1_320,
        jour(2026, 2, 15),
    )
    .await;
    banc.inserer(
        &compte,
        "p-mar",
        "NETFLIX ABONNEMENT",
        -1_280,
        jour(2026, 3, 15),
    )
    .await;

    let source = SourceProgrammable::vide();
    let rapport = banc
        .service(source)
        .executer()
        .await
        .expect("cycle synchro");
    assert_eq!(rapport.comptes_synchronises, 1);
    assert_eq!(
        rapport.transactions_inserees, 0,
        "aucune transaction insérée"
    );

    for (recurrente, intervalle) in banc.recurrences(&compte).await {
        assert!(
            !recurrente,
            "sans insertion, le recalcul n'est pas déclenché : rien n'est marqué"
        );
        assert_eq!(intervalle, None);
    }

    banc.detruire().await;
}
