use crate::domain::balance::{Balance, NouvelleBalance};
use crate::domain::bank_account::{
    BankAccount, BankAccountId, CompteASynchroniser, PlanificationSynchro,
};
use crate::domain::compte::ProprietaireId;
use crate::domain::consent::{Consent, ConsentId, ConsentStatus};
use crate::domain::horloge::Horloge;
use crate::domain::ports::bank_data_source::{BankDataSource, BankDataSourceError};
use crate::domain::ports::ecriture::{
    BalancesWriteRepository, BankTransactionsWriteRepository, ConsentsStatutWriteRepository,
    EcritureError, PlanificationSynchroWriteRepository, ResultatInsertion,
};
use crate::domain::ports::evenement_synchro::{EvenementSynchro, EventPublisher};
use crate::domain::ports::lecture::{
    CompteEcheant, LectureError, PlanificationSynchroReadRepository,
};
use crate::domain::transaction_bancaire::{NouvelleTransactionBancaire, TransactionBancaire};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use std::sync::Arc;

pub const QUOTA_JOURNALIER_DEFAUT: i32 = 4;

#[derive(Debug, thiserror::Error)]
pub enum SynchroError {
    #[error("lecture des comptes à synchroniser impossible : {0}")]
    Lecture(#[from] LectureError),
    #[error("persistance de la synchro impossible : {0}")]
    Ecriture(#[from] EcritureError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RapportSynchro {
    pub comptes_evalues: u64,
    pub comptes_synchronises: u64,
    pub comptes_ignores_quota: u64,
    pub consentements_expires: u64,
    pub transactions_inserees: u64,
    pub transactions_doublons: u64,
    pub soldes_enregistres: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct ParametresSynchro {
    pub quota_journalier: i32,
    pub intervalle: Duration,
    pub fenetre_transactions: Duration,
    pub marge_renouvellement: Duration,
}

impl Default for ParametresSynchro {
    fn default() -> Self {
        Self {
            quota_journalier: QUOTA_JOURNALIER_DEFAUT,
            intervalle: Duration::hours(6),
            fenetre_transactions: Duration::days(30),
            marge_renouvellement: Duration::days(7),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct RemonteeCompte {
    transactions_inserees: u64,
    solde_mis_a_jour: bool,
}

pub struct DependancesSynchro<R, W, S, B, T, C, H, P>
where
    R: PlanificationSynchroReadRepository,
    W: PlanificationSynchroWriteRepository,
    S: BankDataSource + ?Sized,
    B: BalancesWriteRepository,
    T: BankTransactionsWriteRepository,
    C: ConsentsStatutWriteRepository,
    H: Horloge,
    P: EventPublisher + ?Sized,
{
    pub planification_lecture: R,
    pub planification_ecriture: W,
    pub source_bancaire: Arc<S>,
    pub soldes: B,
    pub transactions: T,
    pub consents_statut: C,
    pub horloge: H,
    pub publisher: Arc<P>,
}

pub struct SynchroComptes<R, W, S, B, T, C, H, P>
where
    R: PlanificationSynchroReadRepository,
    W: PlanificationSynchroWriteRepository,
    S: BankDataSource + ?Sized,
    B: BalancesWriteRepository,
    T: BankTransactionsWriteRepository,
    C: ConsentsStatutWriteRepository,
    H: Horloge,
    P: EventPublisher + ?Sized,
{
    planification_lecture: R,
    planification_ecriture: W,
    source_bancaire: Arc<S>,
    soldes: B,
    transactions: T,
    consents_statut: C,
    horloge: H,
    publisher: Arc<P>,
    parametres: ParametresSynchro,
}

impl<R, W, S, B, T, C, H, P> SynchroComptes<R, W, S, B, T, C, H, P>
where
    R: PlanificationSynchroReadRepository,
    W: PlanificationSynchroWriteRepository,
    S: BankDataSource + ?Sized,
    B: BalancesWriteRepository,
    T: BankTransactionsWriteRepository,
    C: ConsentsStatutWriteRepository,
    H: Horloge,
    P: EventPublisher + ?Sized,
{
    pub fn new(
        dependances: DependancesSynchro<R, W, S, B, T, C, H, P>,
        parametres: ParametresSynchro,
    ) -> Self {
        let DependancesSynchro {
            planification_lecture,
            planification_ecriture,
            source_bancaire,
            soldes,
            transactions,
            consents_statut,
            horloge,
            publisher,
        } = dependances;
        Self {
            planification_lecture,
            planification_ecriture,
            source_bancaire,
            soldes,
            transactions,
            consents_statut,
            horloge,
            publisher,
            parametres,
        }
    }

    pub async fn executer(&self) -> Result<RapportSynchro, SynchroError> {
        let maintenant = self.horloge.maintenant();
        let echeants = self
            .planification_lecture
            .lister_comptes_echeants(maintenant, self.parametres.quota_journalier)
            .await?;

        let mut rapport = RapportSynchro::default();
        for echeant in echeants {
            rapport.comptes_evalues += 1;
            self.synchroniser_compte(echeant, maintenant, &mut rapport)
                .await?;
        }

        Ok(rapport)
    }

    async fn synchroniser_compte(
        &self,
        echeant: CompteEcheant,
        maintenant: DateTime<Utc>,
        rapport: &mut RapportSynchro,
    ) -> Result<(), SynchroError> {
        let CompteEcheant { compte, consent } = echeant;
        let proprietaire = compte.proprietaire.clone();
        let reference_compte = compte.id.0.to_string();

        if consentement_expire(&consent, maintenant) {
            self.consents_statut
                .marquer_statut(&consent.id, ConsentStatus::Expired)
                .await?;
            rapport.consentements_expires += 1;
            self.publier(EvenementSynchro::consent_expired(proprietaire, maintenant))
                .await;
            return Ok(());
        }

        let plan = self.plan_apres_synchro(maintenant);
        let creneau_reserve = self
            .planification_ecriture
            .reserver_creneau(&compte.id, plan, self.parametres.quota_journalier)
            .await?;
        if !creneau_reserve {
            rapport.comptes_ignores_quota += 1;
            return Ok(());
        }

        self.publier(EvenementSynchro::sync_started(
            proprietaire.clone(),
            reference_compte.clone(),
            maintenant,
        ))
        .await;

        if renouvellement_requis(&consent, maintenant, self.parametres.marge_renouvellement) {
            self.publier(EvenementSynchro::consent_renewal_required(
                proprietaire.clone(),
                maintenant,
            ))
            .await;
        }

        let vue = vue_bank_account(&compte, maintenant);
        match self.remonter_donnees(&consent, &vue, rapport).await {
            Ok(remontee) => {
                self.publier_succes(&proprietaire, &reference_compte, &remontee, maintenant)
                    .await;
                rapport.comptes_synchronises += 1;
                Ok(())
            }
            Err(erreur) => {
                self.publier(EvenementSynchro::sync_failed(
                    proprietaire.clone(),
                    reference_compte,
                    maintenant,
                ))
                .await;
                self.traiter_echec_source(&proprietaire, &consent.id, &erreur, maintenant, rapport)
                    .await?;
                Ok(())
            }
        }
    }

    async fn remonter_donnees(
        &self,
        consent: &Consent,
        compte: &BankAccount,
        rapport: &mut RapportSynchro,
    ) -> Result<RemonteeCompte, BankDataSourceError> {
        let depuis = self.depuis_pour(compte);
        let mut remontee = RemonteeCompte::default();

        let soldes = self.source_bancaire.solde(consent, compte).await?;
        for solde in soldes {
            if self
                .soldes
                .enregistrer(vers_nouvelle_balance(solde))
                .await
                .is_ok()
            {
                rapport.soldes_enregistres += 1;
                remontee.solde_mis_a_jour = true;
            }
        }

        let transactions = self
            .source_bancaire
            .lister_transactions(consent, compte, depuis)
            .await?;
        for transaction in transactions {
            match self
                .transactions
                .enregistrer(vers_nouvelle_transaction(&compte.id, transaction))
                .await
            {
                Ok(ResultatInsertion::Inseree(_)) => {
                    rapport.transactions_inserees += 1;
                    remontee.transactions_inserees += 1;
                }
                Ok(ResultatInsertion::Doublon) => rapport.transactions_doublons += 1,
                Err(erreur) => {
                    tracing::warn!(cause = %erreur, "persistance d'une transaction ignorée");
                }
            }
        }

        Ok(remontee)
    }

    async fn publier_succes(
        &self,
        proprietaire: &ProprietaireId,
        reference_compte: &str,
        remontee: &RemonteeCompte,
        maintenant: DateTime<Utc>,
    ) {
        if remontee.solde_mis_a_jour {
            self.publier(EvenementSynchro::balance_updated(
                proprietaire.clone(),
                reference_compte.to_string(),
                maintenant,
            ))
            .await;
        }
        self.publier(EvenementSynchro::account_transactions(
            proprietaire.clone(),
            reference_compte.to_string(),
            remontee.transactions_inserees,
            maintenant,
        ))
        .await;
        self.publier(EvenementSynchro::sync_succeeded(
            proprietaire.clone(),
            reference_compte.to_string(),
            remontee.transactions_inserees,
            maintenant,
        ))
        .await;
    }

    async fn traiter_echec_source(
        &self,
        proprietaire: &ProprietaireId,
        consent: &ConsentId,
        erreur: &BankDataSourceError,
        maintenant: DateTime<Utc>,
        rapport: &mut RapportSynchro,
    ) -> Result<(), SynchroError> {
        if let BankDataSourceError::ConsentementInvalide = erreur {
            self.consents_statut
                .marquer_statut(consent, ConsentStatus::Expired)
                .await?;
            rapport.consentements_expires += 1;
            self.publier(EvenementSynchro::consent_expired(
                proprietaire.clone(),
                maintenant,
            ))
            .await;
        } else {
            tracing::warn!(cause = %erreur, "remontée bancaire en échec, créneau déjà réservé");
        }
        Ok(())
    }

    async fn publier(&self, evenement: EvenementSynchro) {
        self.publisher.publier(evenement).await;
    }

    fn plan_apres_synchro(&self, maintenant: DateTime<Utc>) -> PlanificationSynchro {
        PlanificationSynchro {
            next_sync_at: maintenant + self.parametres.intervalle,
            last_sync_day: maintenant.date_naive(),
            last_sync_at: maintenant,
        }
    }

    fn depuis_pour(&self, compte: &BankAccount) -> NaiveDate {
        let reference = compte.next_sync_at.unwrap_or(compte.created_at);
        (reference - self.parametres.fenetre_transactions).date_naive()
    }
}

fn consentement_expire(consent: &Consent, maintenant: DateTime<Utc>) -> bool {
    if consent.status != ConsentStatus::Active {
        return true;
    }
    matches!(consent.expires_at, Some(expiration) if expiration <= maintenant)
}

fn renouvellement_requis(consent: &Consent, maintenant: DateTime<Utc>, marge: Duration) -> bool {
    matches!(
        consent.expires_at,
        Some(expiration) if expiration > maintenant && expiration <= maintenant + marge
    )
}

fn vue_bank_account(compte: &CompteASynchroniser, maintenant: DateTime<Utc>) -> BankAccount {
    BankAccount {
        id: compte.id.clone(),
        proprietaire: compte.proprietaire.clone(),
        consent: compte.consent.clone(),
        external_account_id: compte.external_account_id.clone(),
        iban_masked: String::new(),
        currency: compte.currency.clone(),
        next_sync_at: Some(maintenant),
        sync_count_today: compte.sync_count_today,
        created_at: maintenant,
        updated_at: maintenant,
    }
}

fn vers_nouvelle_balance(solde: Balance) -> NouvelleBalance {
    NouvelleBalance {
        bank_account: solde.bank_account,
        balance_type: solde.balance_type,
        amount_cents: solde.amount_cents,
        currency: solde.currency,
        reference_date: solde.reference_date,
    }
}

fn vers_nouvelle_transaction(
    compte: &BankAccountId,
    transaction: TransactionBancaire,
) -> NouvelleTransactionBancaire {
    NouvelleTransactionBancaire {
        bank_account: compte.clone(),
        external_transaction_id: transaction.external_transaction_id,
        status: transaction.status,
        label: transaction.label,
        amount_cents: transaction.amount_cents,
        currency: transaction.currency,
        booking_date: transaction.booking_date,
        value_date: transaction.value_date,
    }
}
