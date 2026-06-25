use ch_api_budgy::config;
use ch_api_budgy::db;
use ch_api_budgy::relay::abonne::AbonneRelay;
use ch_api_budgy::relay::handler::UserDeletedHandler;
use ch_api_budgy::routes;
use ch_api_budgy::state::AppState;
use std::sync::Arc;

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

    if let Err(e) = resultat {
        eprintln!("Erreur serveur : {e}");
        std::process::exit(1);
    }
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
