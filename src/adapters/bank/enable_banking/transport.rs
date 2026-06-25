use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodeHttp {
    Get,
    Post,
    Delete,
}

#[derive(Clone)]
pub struct RequeteHttp {
    pub methode: MethodeHttp,
    pub chemin: String,
    pub jeton: String,
    pub corps_json: Option<String>,
}

impl std::fmt::Debug for RequeteHttp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequeteHttp")
            .field("methode", &self.methode)
            .field("chemin", &self.chemin)
            .field("jeton", &"***")
            .field("corps_json", &self.corps_json)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct ReponseHttp {
    pub statut: u16,
    pub corps: String,
}

impl ReponseHttp {
    pub fn est_succes(&self) -> bool {
        (200..300).contains(&self.statut)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("échec réseau vers la source bancaire : {0}")]
    Reseau(String),
}

#[async_trait]
pub trait TransportHttp: Send + Sync {
    async fn envoyer(&self, requete: RequeteHttp) -> Result<ReponseHttp, TransportError>;
}

#[async_trait]
impl<T: TransportHttp> TransportHttp for std::sync::Arc<T> {
    async fn envoyer(&self, requete: RequeteHttp) -> Result<ReponseHttp, TransportError> {
        (**self).envoyer(requete).await
    }
}

pub struct ReqwestTransport {
    base_url: String,
    client: reqwest::Client,
}

impl ReqwestTransport {
    pub fn nouveau(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl TransportHttp for ReqwestTransport {
    async fn envoyer(&self, requete: RequeteHttp) -> Result<ReponseHttp, TransportError> {
        let url = format!("{}{}", self.base_url, requete.chemin);
        let mut builder = match requete.methode {
            MethodeHttp::Get => self.client.get(&url),
            MethodeHttp::Post => self.client.post(&url),
            MethodeHttp::Delete => self.client.delete(&url),
        }
        .bearer_auth(&requete.jeton);

        if let Some(corps) = requete.corps_json {
            builder = builder
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(corps);
        }

        let reponse = builder
            .send()
            .await
            .map_err(|e| TransportError::Reseau(e.to_string()))?;
        let statut = reponse.status().as_u16();
        let corps = reponse
            .text()
            .await
            .map_err(|e| TransportError::Reseau(e.to_string()))?;

        Ok(ReponseHttp { statut, corps })
    }
}
