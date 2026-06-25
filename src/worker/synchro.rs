use crate::crypto::CryptoService;
use crate::db::Db;
use crate::domain::horloge::{Horloge, HorlogeSysteme};
use crate::domain::ports::bank_data_source::BankDataSource;
use crate::domain::ports::ecriture::{
    BalancesWriteRepository, BankTransactionsWriteRepository, ConsentsStatutWriteRepository,
    PlanificationSynchroWriteRepository,
};
use crate::domain::ports::lecture::PlanificationSynchroReadRepository;
use crate::domain::synchro::{
    DependancesSynchro, ParametresSynchro, RapportSynchro, SynchroComptes, SynchroError,
};
use crate::repository::balances::SqlxBalancesWriteAdapter;
use crate::repository::bank_accounts::SqlxBankAccountsWriteAdapter;
use crate::repository::bank_transactions::SqlxBankTransactionsWriteAdapter;
use crate::repository::consents::SqlxConsentsWriteAdapter;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

pub type ServiceSynchroSqlx = SynchroComptes<
    SqlxBankAccountsWriteAdapter,
    SqlxBankAccountsWriteAdapter,
    dyn BankDataSource,
    SqlxBalancesWriteAdapter,
    SqlxBankTransactionsWriteAdapter,
    SqlxConsentsWriteAdapter,
    HorlogeSysteme,
>;

pub fn construire_service_synchro(
    db: Db,
    crypto: Arc<CryptoService>,
    bank_source: Arc<dyn BankDataSource>,
    parametres: ParametresSynchro,
) -> ServiceSynchroSqlx {
    let comptes = SqlxBankAccountsWriteAdapter::new(db.clone(), crypto.clone());
    let soldes = SqlxBalancesWriteAdapter::new(db.clone(), crypto.clone());
    let transactions = SqlxBankTransactionsWriteAdapter::new(db.clone(), crypto.clone());
    let consents = SqlxConsentsWriteAdapter::new(db, crypto);

    let dependances = DependancesSynchro {
        planification_lecture: comptes.clone(),
        planification_ecriture: comptes,
        source_bancaire: bank_source,
        soldes,
        transactions,
        consents_statut: consents,
        horloge: HorlogeSysteme,
    };

    SynchroComptes::new(dependances, parametres)
}

pub trait CycleSynchro: Send + Sync + 'static {
    fn executer_cycle(
        &self,
    ) -> impl Future<Output = Result<RapportSynchro, SynchroError>> + Send;
}

impl<R, W, S, B, T, C, H> CycleSynchro for SynchroComptes<R, W, S, B, T, C, H>
where
    R: PlanificationSynchroReadRepository + 'static,
    W: PlanificationSynchroWriteRepository + 'static,
    S: BankDataSource + ?Sized + 'static,
    B: BalancesWriteRepository + 'static,
    T: BankTransactionsWriteRepository + 'static,
    C: ConsentsStatutWriteRepository + 'static,
    H: Horloge + 'static,
{
    fn executer_cycle(
        &self,
    ) -> impl Future<Output = Result<RapportSynchro, SynchroError>> + Send {
        self.executer()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WorkerSynchroConfig {
    pub enabled: bool,
    pub intervalle: Duration,
}

impl Default for WorkerSynchroConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            intervalle: Duration::from_secs(6 * 60 * 60),
        }
    }
}

pub struct WorkerSynchro {
    handle: JoinHandle<()>,
    arret: Arc<Notify>,
}

impl WorkerSynchro {
    pub fn demarrer<C>(config: WorkerSynchroConfig, cycle: Arc<C>) -> Option<Self>
    where
        C: CycleSynchro,
    {
        if !config.enabled {
            tracing::info!("worker de synchro désactivé");
            return None;
        }

        let arret = Arc::new(Notify::new());
        let arret_tache = arret.clone();
        let intervalle = config.intervalle;

        let handle = tokio::spawn(async move {
            tracing::info!(
                intervalle_secondes = intervalle.as_secs(),
                "worker de synchro démarré"
            );
            let mut horloge = tokio::time::interval(intervalle);
            horloge.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    _ = horloge.tick() => {
                        executer_un_cycle(cycle.as_ref()).await;
                    }
                    _ = arret_tache.notified() => {
                        tracing::info!("worker de synchro arrêté");
                        break;
                    }
                }
            }
        });

        Some(Self { handle, arret })
    }

    pub async fn arreter(self) {
        self.arret.notify_one();
        if let Err(erreur) = self.handle.await {
            tracing::warn!(cause = %erreur, "arrêt du worker de synchro non propre");
        }
    }
}

async fn executer_un_cycle<C>(cycle: &C)
where
    C: CycleSynchro,
{
    match cycle.executer_cycle().await {
        Ok(rapport) => {
            tracing::info!(
                comptes_evalues = rapport.comptes_evalues,
                comptes_synchronises = rapport.comptes_synchronises,
                comptes_ignores_quota = rapport.comptes_ignores_quota,
                consentements_expires = rapport.consentements_expires,
                transactions_inserees = rapport.transactions_inserees,
                transactions_doublons = rapport.transactions_doublons,
                soldes_enregistres = rapport.soldes_enregistres,
                "cycle de synchro terminé"
            );
        }
        Err(erreur) => {
            tracing::error!(cause = %erreur, "cycle de synchro en échec");
        }
    }
}
