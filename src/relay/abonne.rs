use crate::config::RelayConfig;
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, QoS, Transport};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum AbonneError {
    #[error("url de broker invalide : {0}")]
    UrlInvalide(String),
}

pub trait MessageHandler: Send + Sync + 'static {
    fn traiter(&self, payload: Vec<u8>) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send>>;
}

#[derive(Debug, Clone)]
pub(crate) struct BrokerAdresse {
    pub hote: String,
    pub port: u16,
    pub tls: bool,
}

pub struct AbonneRelay {
    handle: tokio::task::JoinHandle<()>,
}

impl AbonneRelay {
    pub fn demarrer<H>(
        config: &RelayConfig,
        token: Option<String>,
        handler: Arc<H>,
    ) -> Result<Option<Self>, AbonneError>
    where
        H: MessageHandler,
    {
        if !config.enabled {
            tracing::info!("abonné Relay dormant : aucune connexion au broker");
            return Ok(None);
        }

        let adresse = parser_url(&config.url)?;
        let mut options = MqttOptions::new(config.client_id.clone(), adresse.hote, adresse.port);
        options.set_keep_alive(Duration::from_secs(30));
        if adresse.tls {
            options.set_transport(Transport::tls_with_default_config());
        }
        if let Some(token) = token {
            options.set_credentials(config.client_id.clone(), token);
        }

        let topic = config.topic_user_deleted.clone();
        let (client, mut event_loop) = AsyncClient::new(options, 16);

        let handle = tokio::spawn(async move {
            if let Err(erreur) = client.subscribe(&topic, QoS::AtLeastOnce).await {
                tracing::error!(cause = %erreur, "abonnement Relay impossible");
                return;
            }
            tracing::info!(topic = %topic, "abonné Relay actif");

            loop {
                match event_loop.poll().await {
                    Ok(Event::Incoming(Incoming::Publish(publication))) => {
                        handler.traiter(publication.payload.to_vec()).await;
                    }
                    Ok(_) => {}
                    Err(erreur) => {
                        tracing::warn!(cause = %erreur, "boucle Relay interrompue, nouvelle tentative");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        });

        Ok(Some(Self { handle }))
    }

    pub fn arreter(self) {
        self.handle.abort();
    }
}

pub(crate) fn parser_url(url: &str) -> Result<BrokerAdresse, AbonneError> {
    let (schema, reste) = url
        .split_once("://")
        .ok_or_else(|| AbonneError::UrlInvalide(url.to_string()))?;

    let tls = match schema {
        "mqtt" | "tcp" => false,
        "mqtts" | "ssl" | "tls" => true,
        _ => return Err(AbonneError::UrlInvalide(url.to_string())),
    };

    let autorite = reste.split('/').next().unwrap_or(reste);
    let (hote, port) = match autorite.rsplit_once(':') {
        Some((h, p)) => {
            let port = p
                .parse::<u16>()
                .map_err(|_| AbonneError::UrlInvalide(url.to_string()))?;
            (h.to_string(), port)
        }
        None => (autorite.to_string(), if tls { 8883 } else { 1883 }),
    };

    if hote.is_empty() {
        return Err(AbonneError::UrlInvalide(url.to_string()));
    }

    Ok(BrokerAdresse { hote, port, tls })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_url_tcp_avec_port() {
        let adresse = parser_url("mqtt://broker.local:1884").expect("url valide");
        assert_eq!(adresse.hote, "broker.local");
        assert_eq!(adresse.port, 1884);
        assert!(!adresse.tls);
    }

    #[test]
    fn parse_url_tls_par_defaut() {
        let adresse = parser_url("mqtts://broker.local").expect("url valide");
        assert_eq!(adresse.port, 8883);
        assert!(adresse.tls);
    }

    #[test]
    fn parse_url_sans_schema_rejetee() {
        assert!(parser_url("broker.local:1883").is_err());
    }
}
