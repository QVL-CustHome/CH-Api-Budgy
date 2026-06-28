use crate::config::RelayConfig;
use crate::domain::ports::evenement_synchro::{EvenementSynchro, EventPublisher};
use crate::relay::abonne::{AbonneError, parser_url};
use crate::relay::signature::{SignataireRs256, SignatureError};
use chrono::Utc;
use rumqttc::{AsyncClient, Event, MqttOptions, QoS, Transport};
use serde::Serialize;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

const TTL_MESSAGE_SECONDES: i64 = 300;

#[derive(Debug, thiserror::Error)]
pub enum PublisherError {
    #[error(transparent)]
    Adresse(#[from] AbonneError),
    #[error(transparent)]
    Signature(#[from] SignatureError),
    #[error("clé privée RS256 absente : publication impossible")]
    ClePriveeManquante,
}

#[derive(Serialize)]
struct EnveloppeEvenement<'a> {
    iss: &'a str,
    sub: &'a str,
    event_type: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    account: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    count: Option<u64>,
    at: String,
    iat: i64,
    exp: i64,
}

pub fn construire_payload(
    evenement: &EvenementSynchro,
    issuer: &str,
    signataire: &SignataireRs256,
) -> Result<String, SignatureError> {
    let maintenant = Utc::now().timestamp();
    let enveloppe = EnveloppeEvenement {
        iss: issuer,
        sub: &evenement.proprietaire.0,
        event_type: evenement.type_evenement.as_str(),
        account: evenement.compte.as_deref(),
        count: evenement.count,
        at: evenement.at.to_rfc3339(),
        iat: maintenant,
        exp: maintenant + TTL_MESSAGE_SECONDES,
    };
    signataire.signer(&enveloppe)
}

pub fn topic_pour(prefixe: &str, evenement: &EvenementSynchro) -> String {
    format!(
        "{prefixe}/{}/{}",
        evenement.proprietaire.0,
        evenement.type_evenement.segment_topic()
    )
}

pub struct PublisherRelay {
    client: AsyncClient,
    signataire: Arc<SignataireRs256>,
    prefixe_topic: String,
    issuer: String,
}

impl PublisherRelay {
    pub fn demarrer(
        config: &RelayConfig,
        token: Option<String>,
        cle_privee_pem: Option<&[u8]>,
    ) -> Result<Option<Arc<Self>>, PublisherError> {
        if !config.enabled {
            tracing::info!("publisher Relay dormant : aucune publication réelle");
            return Ok(None);
        }

        let pem = cle_privee_pem.ok_or(PublisherError::ClePriveeManquante)?;
        let signataire = Arc::new(SignataireRs256::nouveau(pem)?);

        let adresse = parser_url(&config.url)?;
        let client_id = format!("{}-pub", config.client_id);
        let mut options = MqttOptions::new(client_id.clone(), adresse.hote, adresse.port);
        options.set_keep_alive(Duration::from_secs(30));
        if adresse.tls {
            options.set_transport(Transport::tls_with_default_config());
        }
        if let Some(token) = token {
            options.set_credentials(client_id, token);
        }

        let (client, mut event_loop) = AsyncClient::new(options, 32);
        tokio::spawn(async move {
            loop {
                match event_loop.poll().await {
                    Ok(Event::Incoming(_)) | Ok(Event::Outgoing(_)) => {}
                    Err(erreur) => {
                        tracing::warn!(cause = %erreur, "boucle de publication Relay interrompue, nouvelle tentative");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        });

        Ok(Some(Arc::new(Self {
            client,
            signataire,
            prefixe_topic: config.topic_prefix.clone(),
            issuer: config.event_issuer.clone(),
        })))
    }

    async fn publier_evenement(&self, evenement: EvenementSynchro) {
        let topic = topic_pour(&self.prefixe_topic, &evenement);
        let retain = evenement.type_evenement.retenu();

        let payload = match construire_payload(&evenement, &self.issuer, &self.signataire) {
            Ok(payload) => payload,
            Err(erreur) => {
                tracing::warn!(cause = %erreur, "signature d'un event de synchro impossible, event ignoré");
                return;
            }
        };

        if let Err(erreur) = self
            .client
            .publish(&topic, QoS::AtLeastOnce, retain, payload.into_bytes())
            .await
        {
            tracing::warn!(cause = %erreur, topic = %topic, "publication d'un event de synchro impossible, ignorée");
        }
    }
}

impl EventPublisher for PublisherRelay {
    fn publier(
        &self,
        evenement: EvenementSynchro,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(self.publier_evenement(evenement))
    }
}
