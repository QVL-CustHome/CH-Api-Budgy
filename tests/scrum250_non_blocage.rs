mod support;

use ch_api_budgy::domain::bank_account::{BankAccountId, CompteASynchroniser, PlanificationSynchro};
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::{Consent, ConsentId, ConsentStatus};
use ch_api_budgy::domain::horloge::Horloge;
use ch_api_budgy::domain::ports::ecriture::{EcritureError, PlanificationSynchroWriteRepository};
use ch_api_budgy::domain::ports::evenement_synchro::{
    EvenementSynchro, EventPublisher, NoopEventPublisher, TypeEvenementSynchro,
};
use ch_api_budgy::domain::ports::lecture::{
    CompteEcheant, LectureError, PlanificationSynchroReadRepository,
};
use ch_api_budgy::domain::synchro::{DependancesSynchro, ParametresSynchro, SynchroComptes};
use chrono::{DateTime, Duration, TimeZone, Utc};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use support::{
    BalancesMemoireStub, ConsentsStatutStub, SourceBancaireFake, TransactionsMemoireStub,
};
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
        etablissement: None,
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
    source: Arc<SourceBancaireFake>,
    publisher: Arc<P>,
) -> SynchroComptes<
    CompteUnique,
    CompteUnique,
    SourceBancaireFake,
    BalancesMemoireStub,
    TransactionsMemoireStub,
    ConsentsStatutStub,
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
        soldes: BalancesMemoireStub,
        transactions: TransactionsMemoireStub,
        consents_statut: ConsentsStatutStub,
        horloge: HorlogeFixe(maintenant()),
        publisher,
    };
    SynchroComptes::new(dependances, ParametresSynchro::default())
}

#[tokio::test]
async fn la_synchro_aboutit_meme_avec_un_publisher_dormant() {
    let consent = consent_actif(Duration::days(30));
    let source = Arc::new(SourceBancaireFake::operationnelle());
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
    let source = Arc::new(SourceBancaireFake::operationnelle());
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
    let source = Arc::new(SourceBancaireFake::operationnelle());
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
    let source = Arc::new(SourceBancaireFake::en_echec());
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
    let source = Arc::new(SourceBancaireFake::operationnelle());
    let espion = Arc::new(PublisherEspion::default());
    let synchro = service(consent, source, espion.clone());

    synchro.executer().await.expect("cycle proche expiration");

    assert!(
        espion
            .types()
            .contains(&TypeEvenementSynchro::ConsentRenewalRequired)
    );
}
