use ch_api_budgy::config::RelayConfig;
use ch_api_budgy::relay::abonne::{AbonneError, AbonneRelay, MessageHandler};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

struct HandlerEspion {
    appels: Arc<AtomicU64>,
}

impl MessageHandler for HandlerEspion {
    fn traiter(&self, _payload: Vec<u8>) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        let appels = self.appels.clone();
        Box::pin(async move {
            appels.fetch_add(1, Ordering::SeqCst);
        })
    }
}

fn config_dormante_vers_broker_mort() -> RelayConfig {
    RelayConfig {
        enabled: false,
        url: "mqtt://127.0.0.1:1".to_string(),
        client_id: "ch-api-budgy-test".to_string(),
        topic_user_deleted: "auth/user/deleted".to_string(),
    }
}

#[tokio::test]
async fn relay_dormant_ne_demarre_aucun_abonne_et_ne_bloque_pas() {
    let appels = Arc::new(AtomicU64::new(0));
    let handler = Arc::new(HandlerEspion {
        appels: appels.clone(),
    });

    let abonne = AbonneRelay::demarrer(&config_dormante_vers_broker_mort(), None, handler)
        .expect("démarrage dormant sans erreur");

    assert!(abonne.is_none());
}

#[tokio::test]
async fn relay_dormant_ne_contacte_jamais_le_broker_ni_le_handler() {
    let appels = Arc::new(AtomicU64::new(0));
    let handler = Arc::new(HandlerEspion {
        appels: appels.clone(),
    });

    let abonne = AbonneRelay::demarrer(&config_dormante_vers_broker_mort(), None, handler)
        .expect("démarrage dormant sans erreur");

    tokio::time::sleep(Duration::from_millis(200)).await;

    assert!(abonne.is_none());
    assert_eq!(appels.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn relay_actif_avec_url_invalide_echoue_proprement_sans_panique() {
    let appels = Arc::new(AtomicU64::new(0));
    let handler = Arc::new(HandlerEspion { appels });

    let config = RelayConfig {
        enabled: true,
        url: "url-sans-schema".to_string(),
        client_id: "ch-api-budgy-test".to_string(),
        topic_user_deleted: "auth/user/deleted".to_string(),
    };

    let resultat = AbonneRelay::demarrer(&config, None, handler);

    assert!(matches!(resultat, Err(AbonneError::UrlInvalide(_))));
}
