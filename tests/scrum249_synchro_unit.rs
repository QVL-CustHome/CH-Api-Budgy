mod support;

use async_trait::async_trait;
use ch_api_budgy::domain::balance::{Balance, BalanceId, BalanceType};
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
    BankTransactionsWriteRepository, ConsentsStatutWriteRepository, EcritureError,
    PlanificationSynchroWriteRepository, ResultatInsertion,
};
use ch_api_budgy::domain::ports::evenement_synchro::NoopEventPublisher;
use ch_api_budgy::domain::ports::lecture::{
    CompteEcheant, LectureError, PlanificationSynchroReadRepository,
};
use ch_api_budgy::domain::synchro::{DependancesSynchro, ParametresSynchro, SynchroComptes};
use ch_api_budgy::domain::transaction_bancaire::{
    NouvelleTransactionBancaire, TransactionBancaire, TransactionBancaireId, TransactionStatus,
};
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use support::BalancesMemoireStub;
use uuid::Uuid;

#[derive(Clone)]
struct HorlogeFixe {
    instant: Arc<Mutex<DateTime<Utc>>>,
}

impl HorlogeFixe {
    fn new(instant: DateTime<Utc>) -> Self {
        Self {
            instant: Arc::new(Mutex::new(instant)),
        }
    }

    fn avancer(&self, delta: Duration) {
        let mut courant = self.instant.lock().expect("horloge non empoisonnée");
        *courant += delta;
    }
}

impl Horloge for HorlogeFixe {
    fn maintenant(&self) -> DateTime<Utc> {
        *self.instant.lock().expect("horloge non empoisonnée")
    }
}

#[derive(Clone)]
struct CompteFixture {
    id: BankAccountId,
    proprietaire: ProprietaireId,
    consent: Consent,
    external_account_id: String,
    currency: String,
    next_sync_at: Option<DateTime<Utc>>,
    sync_count_today: i32,
    last_sync_day: Option<NaiveDate>,
}

#[derive(Clone, Default)]
struct EtatComptes {
    comptes: Arc<Mutex<Vec<CompteFixture>>>,
}

impl EtatComptes {
    fn avec(comptes: Vec<CompteFixture>) -> Self {
        Self {
            comptes: Arc::new(Mutex::new(comptes)),
        }
    }

    fn compte(&self, id: &BankAccountId) -> CompteFixture {
        self.comptes
            .lock()
            .expect("état comptes")
            .iter()
            .find(|c| c.id == *id)
            .cloned()
            .expect("compte présent")
    }
}

impl PlanificationSynchroReadRepository for EtatComptes {
    async fn lister_comptes_echeants(
        &self,
        maintenant: DateTime<Utc>,
        quota_journalier: i32,
    ) -> Result<Vec<CompteEcheant>, LectureError> {
        let jour = maintenant.date_naive();
        let comptes = self.comptes.lock().expect("état comptes");
        let echeants = comptes
            .iter()
            .filter(|c| c.next_sync_at.map(|n| n <= maintenant).unwrap_or(true))
            .filter(|c| c.consent.status == ConsentStatus::Active)
            .filter(|c| c.consent.expires_at.map(|e| e > maintenant).unwrap_or(true))
            .filter(|c| compteur_effectif(c, jour) < quota_journalier)
            .map(|c| CompteEcheant {
                compte: CompteASynchroniser {
                    id: c.id.clone(),
                    proprietaire: c.proprietaire.clone(),
                    consent: c.consent.id.clone(),
                    external_account_id: c.external_account_id.clone(),
                    currency: c.currency.clone(),
                    sync_count_today: c.sync_count_today,
                    last_sync_day: c.last_sync_day,
                },
                consent: c.consent.clone(),
            })
            .collect();
        Ok(echeants)
    }
}

impl PlanificationSynchroWriteRepository for EtatComptes {
    async fn reserver_creneau(
        &self,
        compte: &BankAccountId,
        plan: PlanificationSynchro,
        quota_journalier: i32,
    ) -> Result<bool, EcritureError> {
        let mut comptes = self.comptes.lock().expect("état comptes");
        let Some(cible) = comptes.iter_mut().find(|c| c.id == *compte) else {
            return Ok(false);
        };
        let compteur_courant = compteur_effectif(cible, plan.last_sync_day);
        if compteur_courant >= quota_journalier {
            return Ok(false);
        }
        cible.next_sync_at = Some(plan.next_sync_at);
        cible.sync_count_today = compteur_courant + 1;
        cible.last_sync_day = Some(plan.last_sync_day);
        Ok(true)
    }
}

fn compteur_effectif(compte: &CompteFixture, jour: NaiveDate) -> i32 {
    match compte.last_sync_day {
        Some(j) if j == jour => compte.sync_count_today,
        _ => 0,
    }
}

#[derive(Clone, Default)]
struct ConsentsStatutMemoire {
    statuts: Arc<Mutex<HashMap<Uuid, ConsentStatus>>>,
}

impl ConsentsStatutMemoire {
    fn statut(&self, consent: &ConsentId) -> Option<ConsentStatus> {
        self.statuts
            .lock()
            .expect("statuts")
            .get(&consent.0)
            .copied()
    }
}

impl ConsentsStatutWriteRepository for ConsentsStatutMemoire {
    async fn marquer_statut(
        &self,
        consent: &ConsentId,
        statut: ConsentStatus,
    ) -> Result<(), EcritureError> {
        self.statuts
            .lock()
            .expect("statuts")
            .insert(consent.0, statut);
        Ok(())
    }
}

#[derive(Clone, Default)]
struct TransactionsMemoire {
    par_dedup: Arc<Mutex<HashMap<String, TransactionStatus>>>,
    appels: Arc<Mutex<u32>>,
}

impl TransactionsMemoire {
    fn statut(&self, cle: &str) -> Option<TransactionStatus> {
        self.par_dedup
            .lock()
            .expect("transactions")
            .get(cle)
            .copied()
    }

    fn appels(&self) -> u32 {
        *self.appels.lock().expect("appels")
    }
}

impl BankTransactionsWriteRepository for TransactionsMemoire {
    async fn enregistrer(
        &self,
        nouvelle: NouvelleTransactionBancaire,
    ) -> Result<ResultatInsertion<TransactionBancaireId>, EcritureError> {
        *self.appels.lock().expect("appels") += 1;
        let cle = format!(
            "{}:{}",
            nouvelle.bank_account.0, nouvelle.external_transaction_id
        );
        let mut table = self.par_dedup.lock().expect("transactions");
        match table.get(&cle).copied() {
            None => {
                table.insert(cle, nouvelle.status);
                Ok(ResultatInsertion::Inseree(TransactionBancaireId(
                    Uuid::new_v4(),
                )))
            }
            Some(TransactionStatus::Pending) if nouvelle.status == TransactionStatus::Booked => {
                table.insert(cle, TransactionStatus::Booked);
                Ok(ResultatInsertion::Doublon)
            }
            Some(_) => Ok(ResultatInsertion::Doublon),
        }
    }
}

struct SourceProgrammable {
    lots: Mutex<Vec<Vec<TransactionBancaire>>>,
    appels_transactions: Arc<Mutex<u32>>,
    consentement_invalide: bool,
}

impl SourceProgrammable {
    fn new(lots: Vec<Vec<TransactionBancaire>>) -> Self {
        Self {
            lots: Mutex::new(lots),
            appels_transactions: Arc::new(Mutex::new(0)),
            consentement_invalide: false,
        }
    }

    fn invalidant() -> Self {
        Self {
            lots: Mutex::new(Vec::new()),
            appels_transactions: Arc::new(Mutex::new(0)),
            consentement_invalide: true,
        }
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
        if self.consentement_invalide {
            return Err(BankDataSourceError::ConsentementInvalide);
        }
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
        *self.appels_transactions.lock().expect("appels") += 1;
        let mut lots = self.lots.lock().expect("lots");
        if lots.is_empty() {
            return Ok(Vec::new());
        }
        Ok(lots.remove(0))
    }

    async fn revoquer_consentement(
        &self,
        consent: &Consent,
    ) -> Result<Consent, BankDataSourceError> {
        Ok(consent.clone())
    }
}

fn base() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 25, 8, 0, 0).unwrap()
}

fn consent_actif(maintenant: DateTime<Utc>) -> Consent {
    Consent {
        id: ConsentId(Uuid::new_v4()),
        proprietaire: ProprietaireId("owner-249".to_string()),
        external_ref: "ref-249".to_string(),
        etablissement: None,
        status: ConsentStatus::Active,
        expires_at: Some(maintenant + Duration::days(30)),
        created_at: maintenant,
        updated_at: maintenant,
    }
}

fn compte_fixture(consent: Consent, echu_a: Option<DateTime<Utc>>) -> CompteFixture {
    CompteFixture {
        id: BankAccountId(Uuid::new_v4()),
        proprietaire: ProprietaireId("owner-249".to_string()),
        consent,
        external_account_id: "acct-249".to_string(),
        currency: "EUR".to_string(),
        next_sync_at: echu_a,
        sync_count_today: 0,
        last_sync_day: None,
    }
}

fn transaction(
    compte: &BankAccountId,
    suffixe: &str,
    status: TransactionStatus,
) -> TransactionBancaire {
    TransactionBancaire {
        id: TransactionBancaireId(Uuid::new_v4()),
        bank_account: compte.clone(),
        external_transaction_id: format!("tx-{suffixe}"),
        status,
        label: "ACHAT".to_string(),
        amount_cents: -1_299,
        currency: "EUR".to_string(),
        booking_date: None,
        value_date: None,
        created_at: Utc.with_ymd_and_hms(2026, 6, 25, 8, 0, 0).unwrap(),
    }
}

type Service = SynchroComptes<
    EtatComptes,
    EtatComptes,
    SourceProgrammable,
    BalancesMemoireStub,
    TransactionsMemoire,
    ConsentsStatutMemoire,
    HorlogeFixe,
    NoopEventPublisher,
>;

struct Banc {
    comptes: EtatComptes,
    transactions: TransactionsMemoire,
    consents: ConsentsStatutMemoire,
    source: Arc<SourceProgrammable>,
    service: Service,
}

fn monter(
    comptes: Vec<CompteFixture>,
    lots: Vec<Vec<TransactionBancaire>>,
    horloge: HorlogeFixe,
    parametres: ParametresSynchro,
) -> Banc {
    monter_avec_source(
        comptes,
        Arc::new(SourceProgrammable::new(lots)),
        horloge,
        parametres,
    )
}

fn monter_avec_source(
    comptes: Vec<CompteFixture>,
    source: Arc<SourceProgrammable>,
    horloge: HorlogeFixe,
    parametres: ParametresSynchro,
) -> Banc {
    let etat = EtatComptes::avec(comptes);
    let transactions = TransactionsMemoire::default();
    let consents = ConsentsStatutMemoire::default();
    let dependances = DependancesSynchro {
        planification_lecture: etat.clone(),
        planification_ecriture: etat.clone(),
        source_bancaire: source.clone(),
        soldes: BalancesMemoireStub,
        transactions: transactions.clone(),
        consents_statut: consents.clone(),
        horloge: horloge.clone(),
        publisher: Arc::new(NoopEventPublisher),
    };
    let service = SynchroComptes::new(dependances, parametres);
    Banc {
        comptes: etat,
        transactions,
        consents,
        source,
        service,
    }
}

#[tokio::test]
async fn seuls_les_comptes_echeants_sont_synchronises() {
    let maintenant = base();
    let horloge = HorlogeFixe::new(maintenant);
    let echu = compte_fixture(
        consent_actif(maintenant),
        Some(maintenant - Duration::hours(1)),
    );
    let non_echu = compte_fixture(
        consent_actif(maintenant),
        Some(maintenant + Duration::hours(2)),
    );
    let banc = monter(
        vec![echu.clone(), non_echu.clone()],
        vec![vec![transaction(&echu.id, "a", TransactionStatus::Booked)]],
        horloge,
        ParametresSynchro::default(),
    );

    let rapport = banc.service.executer().await.expect("cycle");

    assert_eq!(rapport.comptes_synchronises, 1);
    assert_eq!(banc.comptes.compte(&non_echu.id).sync_count_today, 0);
    assert_eq!(banc.comptes.compte(&echu.id).sync_count_today, 1);
}

#[tokio::test]
async fn le_quota_journalier_est_un_invariant() {
    let maintenant = base();
    let horloge = HorlogeFixe::new(maintenant);
    let compte = compte_fixture(
        consent_actif(maintenant),
        Some(maintenant - Duration::hours(1)),
    );
    let parametres = ParametresSynchro {
        intervalle: Duration::seconds(0),
        ..ParametresSynchro::default()
    };
    let banc = monter(
        vec![compte.clone()],
        (0..10).map(|_| Vec::new()).collect(),
        horloge,
        parametres,
    );

    for _ in 0..10 {
        banc.service.executer().await.expect("cycle");
    }

    assert!(banc.comptes.compte(&compte.id).sync_count_today <= 4);
    assert_eq!(banc.comptes.compte(&compte.id).sync_count_today, 4);
}

#[tokio::test]
async fn le_rejeu_respecte_next_sync_at() {
    let maintenant = base();
    let horloge = HorlogeFixe::new(maintenant);
    let compte = compte_fixture(
        consent_actif(maintenant),
        Some(maintenant - Duration::hours(1)),
    );
    let banc = monter(
        vec![compte.clone()],
        vec![
            vec![transaction(&compte.id, "a", TransactionStatus::Booked)],
            vec![transaction(&compte.id, "a", TransactionStatus::Booked)],
        ],
        horloge,
        ParametresSynchro::default(),
    );

    banc.service.executer().await.expect("premier cycle");
    let rapport = banc.service.executer().await.expect("rejeu cycle");

    assert_eq!(
        rapport.comptes_evalues, 0,
        "compte non échu au rejeu immédiat"
    );
    assert_eq!(*banc.source.appels_transactions.lock().unwrap(), 1);
    assert_eq!(banc.comptes.compte(&compte.id).sync_count_today, 1);
}

#[tokio::test]
async fn la_transition_pending_vers_booked_est_appliquee_au_prochain_creneau() {
    let maintenant = base();
    let horloge = HorlogeFixe::new(maintenant);
    let compte = compte_fixture(
        consent_actif(maintenant),
        Some(maintenant - Duration::hours(1)),
    );
    let banc = monter(
        vec![compte.clone()],
        vec![
            vec![transaction(&compte.id, "achat", TransactionStatus::Pending)],
            vec![transaction(&compte.id, "achat", TransactionStatus::Booked)],
        ],
        horloge.clone(),
        ParametresSynchro::default(),
    );

    banc.service.executer().await.expect("premier cycle");
    assert_eq!(
        banc.transactions
            .statut(&format!("{}:tx-achat", compte.id.0)),
        Some(TransactionStatus::Pending)
    );

    horloge.avancer(Duration::hours(7));
    let rapport = banc.service.executer().await.expect("second cycle");

    assert_eq!(rapport.comptes_synchronises, 1);
    assert_eq!(rapport.transactions_doublons, 1);
    assert_eq!(
        banc.transactions
            .statut(&format!("{}:tx-achat", compte.id.0)),
        Some(TransactionStatus::Booked)
    );
    assert_eq!(banc.transactions.appels(), 2);
}

#[tokio::test]
async fn le_consentement_expire_est_exclu_de_la_selection() {
    let maintenant = base();
    let horloge = HorlogeFixe::new(maintenant);
    let mut consent = consent_actif(maintenant);
    consent.expires_at = Some(maintenant - Duration::hours(1));
    let compte = compte_fixture(consent, Some(maintenant - Duration::hours(1)));
    let banc = monter(
        vec![compte.clone()],
        vec![vec![transaction(
            &compte.id,
            "a",
            TransactionStatus::Booked,
        )]],
        horloge,
        ParametresSynchro::default(),
    );

    let rapport = banc.service.executer().await.expect("cycle");

    assert_eq!(rapport.comptes_evalues, 0);
    assert_eq!(rapport.comptes_synchronises, 0);
    assert_eq!(*banc.source.appels_transactions.lock().unwrap(), 0);
}

#[tokio::test]
async fn le_consentement_invalide_cote_fournisseur_est_marque_expire() {
    let maintenant = base();
    let horloge = HorlogeFixe::new(maintenant);
    let compte = compte_fixture(
        consent_actif(maintenant),
        Some(maintenant - Duration::hours(1)),
    );
    let consent_id = compte.consent.id.clone();
    let banc = monter_avec_source(
        vec![compte.clone()],
        Arc::new(SourceProgrammable::invalidant()),
        horloge,
        ParametresSynchro::default(),
    );

    let rapport = banc.service.executer().await.expect("cycle");

    assert_eq!(rapport.comptes_synchronises, 0);
    assert_eq!(rapport.consentements_expires, 1);
    assert_eq!(
        banc.consents.statut(&consent_id),
        Some(ConsentStatus::Expired)
    );
}

#[tokio::test]
async fn le_compteur_est_reinitialise_au_changement_de_jour() {
    let maintenant = base();
    let horloge = HorlogeFixe::new(maintenant);
    let mut compte = compte_fixture(
        consent_actif(maintenant + Duration::days(2)),
        Some(maintenant - Duration::hours(1)),
    );
    compte.sync_count_today = 4;
    compte.last_sync_day = Some(maintenant.date_naive());
    let banc = monter(
        vec![compte.clone()],
        vec![Vec::new(), Vec::new()],
        horloge.clone(),
        ParametresSynchro::default(),
    );

    let rapport_jour1 = banc.service.executer().await.expect("cycle jour 1");
    assert_eq!(rapport_jour1.comptes_evalues, 0, "quota atteint le jour 1");

    horloge.avancer(Duration::days(1));
    let rapport_jour2 = banc.service.executer().await.expect("cycle jour 2");

    assert_eq!(
        rapport_jour2.comptes_synchronises, 1,
        "reset au changement de jour"
    );
    assert_eq!(banc.comptes.compte(&compte.id).sync_count_today, 1);
    assert_eq!(
        banc.comptes.compte(&compte.id).last_sync_day,
        Some((maintenant + Duration::days(1)).date_naive())
    );
}
