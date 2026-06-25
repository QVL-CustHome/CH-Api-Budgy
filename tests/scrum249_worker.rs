use ch_api_budgy::domain::synchro::{RapportSynchro, SynchroError};
use ch_api_budgy::worker::{CycleSynchro, WorkerSynchro, WorkerSynchroConfig};
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

#[derive(Default)]
struct CycleCompteur {
    executions: AtomicU32,
}

impl CycleSynchro for CycleCompteur {
    fn executer_cycle(
        &self,
    ) -> impl Future<Output = Result<RapportSynchro, SynchroError>> + Send {
        self.executions.fetch_add(1, Ordering::SeqCst);
        async { Ok(RapportSynchro::default()) }
    }
}

#[tokio::test]
async fn le_worker_desactive_ne_demarre_pas() {
    let cycle = Arc::new(CycleCompteur::default());
    let config = WorkerSynchroConfig {
        enabled: false,
        intervalle: Duration::from_millis(10),
    };

    let worker = WorkerSynchro::demarrer(config, cycle.clone());

    assert!(worker.is_none());
    assert_eq!(cycle.executions.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn le_worker_execute_des_cycles_puis_s_arrete_proprement() {
    let cycle = Arc::new(CycleCompteur::default());
    let config = WorkerSynchroConfig {
        enabled: true,
        intervalle: Duration::from_millis(20),
    };

    let worker = WorkerSynchro::demarrer(config, cycle.clone()).expect("worker démarré");
    tokio::time::sleep(Duration::from_millis(70)).await;
    worker.arreter().await;

    let executions = cycle.executions.load(Ordering::SeqCst);
    assert!(executions >= 1, "au moins un cycle exécuté, observé {executions}");

    let stable = cycle.executions.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert_eq!(
        cycle.executions.load(Ordering::SeqCst),
        stable,
        "aucun cycle après l'arrêt"
    );
}
