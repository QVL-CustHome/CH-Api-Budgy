use async_trait::async_trait;
use ch_api_budgy::domain::balance::{Balance, BalanceId, BalanceType, NouvelleBalance};
use ch_api_budgy::domain::bank_account::{
    BankAccount, BankAccountId, CompteASynchroniser, PlanificationSynchro,
};
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::{Consent, ConsentId, ConsentStatus};
use ch_api_budgy::domain::horloge::Horloge;
use ch_api_budgy::domain::ports::bank_data_source::{
    BankDataSource, BankDataSourceError, ConsentementInitie, DemandeConsentement, Etablissement,
    ReponseAutorisation,
};
use ch_api_budgy::domain::ports::ecriture::{
    BalancesWriteRepository, BankTransactionsWriteRepository, ConsentsStatutWriteRepository,
    EcritureError, PlanificationSynchroWriteRepository, ResultatInsertion,
};
use ch_api_budgy::domain::ports::evenement_synchro::{
    EvenementSynchro, EventPublisher, NoopEventPublisher, TypeEvenementSynchro,
};
use ch_api_budgy::domain::ports::lecture::{
    CompteEcheant, LectureError, PlanificationSynchroReadRepository,
};
use ch_api_budgy::domain::synchro::{DependancesSynchro, ParametresSynchro, SynchroComptes};
use ch_api_budgy::domain::transaction_bancaire::{
    NouvelleTransactionBancaire, TransactionBancaire, TransactionBancaireId, TransactionStatus,
};
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Clone)]
struct HorlogeFixe(DateTime<Utc>);

impl Horloge for HorlogeFixe {
    fn maintenant(&self) -> DateTime<Utc> {
        self.0
    }
}

#[derive(Clone)]
struct CompteUnique {
    compte: CompteASynchroniser,
    consent: Consent,
}

impl PlanificationSynchroReadRepository for CompteUnique {
    async fn lister_comptes_echeants(
        &self,
        _maintenant: DateTime<Utc>,
        _quota: i32,
    ) -> Result<Vec<CompteEcheant>, LectureError> {
        Ok(vec![CompteEcheant {
            compte: self.compte.clone(),
            consent: self.consent.clone(),
        }])
    }
}

impl PlanificationSynchroWriteRepository for CompteUnique {
    async fn reserver_creneau(
        &self,
        _compte: &BankAccountId,
        _plan: PlanificationSynchro,
        _quota: i32,
    ) -> Result<bool, EcritureError> {
        Ok(true)
    }
}

#[derive(Clone, Default)]
struct ConsentsMemoire;

impl ConsentsStatutWriteRepository for ConsentsMemoire {
    async fn marquer_statut(
        &self,
        _consent: &ConsentId,
        _statut: ConsentStatus,
    ) -> Result<(), EcritureError> {
        Ok(())
    }
}

#[derive(Clone, Default)]
struct BalancesMemoire;

impl BalancesWriteRepository for BalancesMemoire {
    async fn enregistrer(&self, _nouvelle: NouvelleBalance) -> Result<BalanceId, EcritureError> {
        Ok(BalanceId(Uuid::new_v4()))
    }
}

#[derive(Clone, Default)]
struct TransactionsMemoire;

impl BankTransactionsWriteRepository for TransactionsMemoire {
    async fn enregistrer(
        &self,
        _nouvelle: NouvelleTransactionBancaire,
    ) -> Result<ResultatInsertion<TransactionBancaireId>, EcritureError> {
        Ok(ResultatInsertion::Inseree(TransactionBancaireId(
            Uuid::new_v4(),
        )))
    }
}

struct SourceSimple {
    en_echec: bool,
}

#[async_trait]
impl BankDataSource for SourceSimple {
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
        if self.en_echec {
            return Err(BankDataSourceError::ConsentementInvalide);
        }
        Ok(vec![Balance {
            id: BalanceId(Uuid::new_v4()),
            bank_account: compte.id.clone(),
            balance_type: BalanceType::Available,
            amount_cents: 100_000,
            currency: compte.currency.clone(),
            reference_date: Utc.with_ymd_and_hms(2026, 6, 27, 0, 0, 0).unwrap(),
            created_at: Utc.with_ymd_and_hms(2026, 6, 27, 0, 0, 0).unwrap(),
        }])
    }

    async fn lister_transactions(
        &self,
        _consent: &Consent,
        compte: &BankAccount,
        _depuis: NaiveDate,
    ) -> Result<Vec<TransactionBancaire>, BankDataSourceError> {
        Ok(vec![TransactionBancaire {
            id: TransactionBancaireId(Uuid::new_v4()),
            bank_account: compte.id.clone(),
            external_transaction_id: "tx-1".to_string(),
            status: TransactionStatus::Booked,
            label: "ACHAT".to_string(),
            amount_cents: -1_299,
            currency: "EUR".to_string(),
            booking_date: None,
            value_date: None,
            created_at: Utc.with_ymd_and_hms(2026, 6, 27, 0, 0, 0).unwrap(),
        }])
    }

    async fn revoquer_consentement(
        &self,
        consent: &Consent,
    ) -> Result<Consent, BankDataSourceError> {
        Ok(consent.clone())
    }
}

#[derive(Clone, Default)]
struct PublisherEspion {
    evenements: Arc<Mutex<Vec<EvenementSynchro>>>,
}

impl PublisherEspion {
    fn types(&self) -> Vec<TypeEvenementSynchro> {
        self.evenements
            .lock()
            .expect("evenements")
            .iter()
            .map(|e| e.type_evenement)
            .collect()
    }
}

impl EventPublisher for PublisherEspion {
    fn publier(
        &self,
        evenement: EvenementSynchro,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            self.evenements.lock().expect("evenements").push(evenement);
        })
    }
}

struct PublisherDefaillant {
    appele: AtomicBool,
}

impl EventPublisher for PublisherDefaillant {
    fn publier(
        &self,
        _evenement: EvenementSynchro,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.appele.store(true, Ordering::SeqCst);
        Box::pin(async {})
    }
}

fn maintenant() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 27, 9, 0, 0).unwrap()
}

fn consent_actif(expire_dans: Duration) -> Consent {
    let base = maintenant();
    Consent {
        id: ConsentId(Uuid::new_v4()),
        proprietaire: ProprietaireId("owner-250".to_string()),
        external_ref: "ref-250".to_string(),
        status: ConsentStatus::Active,
        expires_at: Some(base + expire_dans),
        created_at: base,
        updated_at: base,
    }
}

fn compte(consent: &Consent) -> CompteASynchroniser {
    CompteASynchroniser {
        id: BankAccountId(Uuid::new_v4()),
        proprietaire: ProprietaireId("owner-250".to_string()),
        consent: consent.id.clone(),
        external_account_id: "acct-250".to_string(),
        currency: "EUR".to_string(),
        sync_count_today: 0,
        last_sync_day: None,
    }
}

fn service<P>(
    consent: Consent,
    source: Arc<SourceSimple>,
    publisher: Arc<P>,
) -> SynchroComptes<
    CompteUnique,
    CompteUnique,
    SourceSimple,
    BalancesMemoire,
    TransactionsMemoire,
    ConsentsMemoire,
    HorlogeFixe,
    P,
>
where
    P: EventPublisher + ?Sized,
{
    let etat = CompteUnique {
        compte: compte(&consent),
        consent,
    };
    let dependances = DependancesSynchro {
        planification_lecture: etat.clone(),
        planification_ecriture: etat,
        source_bancaire: source,
        soldes: BalancesMemoire,
        transactions: TransactionsMemoire,
        consents_statut: ConsentsMemoire,
        horloge: HorlogeFixe(maintenant()),
        publisher,
    };
    SynchroComptes::new(dependances, ParametresSynchro::default())
}

#[tokio::test]
async fn la_synchro_aboutit_meme_avec_un_publisher_dormant() {
    let consent = consent_actif(Duration::days(30));
    let source = Arc::new(SourceSimple { en_echec: false });
    let publisher = Arc::new(NoopEventPublisher);
    let synchro = service(consent, source, publisher);

    let rapport = synchro.executer().await.expect("la synchro aboutit");

    assert_eq!(rapport.comptes_synchronises, 1);
    assert_eq!(rapport.transactions_inserees, 1);
    assert_eq!(rapport.soldes_enregistres, 1);
}

#[tokio::test]
async fn la_synchro_aboutit_meme_si_le_publisher_est_sollicite() {
    let consent = consent_actif(Duration::days(30));
    let source = Arc::new(SourceSimple { en_echec: false });
    let publisher = Arc::new(PublisherDefaillant {
        appele: AtomicBool::new(false),
    });
    let synchro = service(consent, source, publisher.clone());

    let rapport = synchro.executer().await.expect("la synchro aboutit");

    assert_eq!(rapport.comptes_synchronises, 1);
    assert!(publisher.appele.load(Ordering::SeqCst));
}

#[tokio::test]
async fn le_cycle_nominal_emet_les_events_attendus() {
    let consent = consent_actif(Duration::days(30));
    let source = Arc::new(SourceSimple { en_echec: false });
    let espion = Arc::new(PublisherEspion::default());
    let synchro = service(consent, source, espion.clone());

    synchro.executer().await.expect("cycle nominal");

    let types = espion.types();
    assert_eq!(types.first(), Some(&TypeEvenementSynchro::SyncStarted));
    assert!(types.contains(&TypeEvenementSynchro::BalanceUpdated));
    assert!(types.contains(&TypeEvenementSynchro::AccountTransactions));
    assert!(types.contains(&TypeEvenementSynchro::SyncSucceeded));
    assert!(!types.contains(&TypeEvenementSynchro::SyncFailed));
}

#[tokio::test]
async fn l_echec_source_emet_sync_failed_et_consent_expired() {
    let consent = consent_actif(Duration::days(30));
    let source = Arc::new(SourceSimple { en_echec: true });
    let espion = Arc::new(PublisherEspion::default());
    let synchro = service(consent, source, espion.clone());

    let rapport = synchro.executer().await.expect("cycle en échec source");

    assert_eq!(rapport.comptes_synchronises, 0);
    let types = espion.types();
    assert!(types.contains(&TypeEvenementSynchro::SyncStarted));
    assert!(types.contains(&TypeEvenementSynchro::SyncFailed));
    assert!(types.contains(&TypeEvenementSynchro::ConsentExpired));
}

#[tokio::test]
async fn le_consentement_proche_de_l_expiration_emet_renewal_required() {
    let consent = consent_actif(Duration::days(3));
    let source = Arc::new(SourceSimple { en_echec: false });
    let espion = Arc::new(PublisherEspion::default());
    let synchro = service(consent, source, espion.clone());

    synchro.executer().await.expect("cycle proche expiration");

    assert!(
        espion
            .types()
            .contains(&TypeEvenementSynchro::ConsentRenewalRequired)
    );
}
