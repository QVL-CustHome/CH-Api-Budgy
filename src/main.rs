use ch_api_budgy::config;
use ch_api_budgy::config::{Settings, WorkerSynchroSettings};
use ch_api_budgy::db;
use ch_api_budgy::domain::ports::evenement_synchro::{EventPublisher, NoopEventPublisher};
use ch_api_budgy::domain::synchro::ParametresSynchro;
use ch_api_budgy::relay::abonne::AbonneRelay;
use ch_api_budgy::relay::handler::UserDeletedHandler;
use ch_api_budgy::relay::publisher::PublisherRelay;
use ch_api_budgy::routes;
use ch_api_budgy::state::AppState;
use ch_api_budgy::worker::WorkerSynchroConfig;
use ch_api_budgy::worker::synchro::construire_service_synchro;
use chrono::Duration as ChronoDuration;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let settings = match config::load("config.toml") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Démarrage impossible — configuration invalide : {e}");
            std::process::exit(1);
        }
    };

    init_tracing(&settings.config.server.log_level);

    let pool = match db::connect(&settings.secrets.database_url).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "PostgreSQL injoignable");
            eprintln!("Démarrage impossible — PostgreSQL injoignable : {e}");
            std::process::exit(1);
        }
    };

    if let Err(e) = db::migrate(&pool).await {
        tracing::error!(error = %e, "Migrations en échec");
        eprintln!("Démarrage impossible — migrations en échec : {e}");
        std::process::exit(1);
    }

    let port = settings.config.server.port;
    let state = AppState::new(&settings, pool);

    let handler = Arc::new(UserDeletedHandler::new(
        state.db.clone(),
        state.crypto.clone(),
        state.bank_source.clone(),
    ));
    let abonne_relay = match AbonneRelay::demarrer(
        &settings.config.relay,
        settings.secrets.relay_token.clone(),
        handler,
    ) {
        Ok(abonne) => abonne,
        Err(e) => {
            tracing::error!(error = %e, "abonné Relay non démarré");
            None
        }
    };

    let publisher = construire_publisher(&settings);
    let worker_synchro =
        demarrer_worker_synchro(&settings.config.worker_synchro, &state, publisher);

    let app = routes::router(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Démarrage impossible — écoute sur {addr} refusée : {e}");
            std::process::exit(1);
        }
    };

    tracing::info!(%addr, version = env!("CARGO_PKG_VERSION"), "CH-Api-Budgy démarré");
    let resultat = axum::serve(listener, app)
        .with_graceful_shutdown(attendre_arret())
        .await;

    if let Some(abonne_relay) = abonne_relay {
        abonne_relay.arreter();
    }

    if let Some(worker_synchro) = worker_synchro {
        worker_synchro.arreter().await;
    }

    if let Err(e) = resultat {
        eprintln!("Erreur serveur : {e}");
        std::process::exit(1);
    }
}

fn construire_publisher(settings: &Settings) -> Arc<dyn EventPublisher> {
    let cle_pem = settings
        .secrets
        .relay_jwt_private_key
        .as_ref()
        .map(|cle| cle.as_bytes());
    match PublisherRelay::demarrer(
        &settings.config.relay,
        settings.secrets.relay_token.clone(),
        cle_pem,
    ) {
        Ok(Some(publisher)) => publisher,
        Ok(None) => Arc::new(NoopEventPublisher),
        Err(e) => {
            tracing::error!(error = %e, "publisher Relay non démarré, repli sur no-op");
            Arc::new(NoopEventPublisher)
        }
    }
}

fn demarrer_worker_synchro(
    settings: &WorkerSynchroSettings,
    state: &AppState,
    publisher: Arc<dyn EventPublisher>,
) -> Option<ch_api_budgy::worker::WorkerSynchro> {
    let parametres = ParametresSynchro {
        quota_journalier: settings.quota_journalier,
        intervalle: ChronoDuration::seconds(settings.interval_secondes as i64),
        fenetre_transactions: ChronoDuration::days(settings.fenetre_transactions_jours),
        ..ParametresSynchro::default()
    };
    let service = construire_service_synchro(
        state.db.clone(),
        state.crypto.clone(),
        state.bank_source.clone(),
        publisher,
        parametres,
    );
    let config = WorkerSynchroConfig {
        enabled: settings.enabled,
        intervalle: Duration::from_secs(settings.interval_secondes),
    };
    ch_api_budgy::worker::WorkerSynchro::demarrer(config, Arc::new(service))
}

async fn attendre_arret() {
    if let Err(e) = tokio::signal::ctrl_c().await {
        tracing::error!(error = %e, "écoute du signal d'arrêt impossible");
    }
}

fn init_tracing(level: &str) {
    let filter = tracing_subscriber::EnvFilter::try_new(level.to_lowercase())
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .init();
}
