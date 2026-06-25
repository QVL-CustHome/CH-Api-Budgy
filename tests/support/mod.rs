#![allow(dead_code)]

use async_trait::async_trait;
use ch_api_budgy::adapters::bank::enable_banking::transport::{
    ReponseHttp, RequeteHttp, TransportError, TransportHttp,
};
use rsa::RsaPrivateKey;
use rsa::pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey, LineEnding};
use std::collections::VecDeque;
use std::sync::Mutex;

pub struct PaireRsaTest {
    pub privee_pem: String,
    pub publique_pem: String,
}

pub fn paire_rsa_test() -> PaireRsaTest {
    let mut rng = rand::thread_rng();
    let privee = RsaPrivateKey::new(&mut rng, 2048).expect("génération clé RSA de test");
    let publique = privee.to_public_key();
    PaireRsaTest {
        privee_pem: privee
            .to_pkcs1_pem(LineEnding::LF)
            .expect("encodage PEM clé privée")
            .to_string(),
        publique_pem: publique
            .to_pkcs1_pem(LineEnding::LF)
            .expect("encodage PEM clé publique"),
    }
}

#[derive(Debug, Clone)]
pub struct EchangeSimule {
    pub statut: u16,
    pub corps: String,
}

impl EchangeSimule {
    pub fn ok(corps: &str) -> Self {
        Self {
            statut: 200,
            corps: corps.to_string(),
        }
    }

    pub fn statut(statut: u16, corps: &str) -> Self {
        Self {
            statut,
            corps: corps.to_string(),
        }
    }
}

pub struct TransportSimule {
    reponses: Mutex<VecDeque<EchangeSimule>>,
    requetes: Mutex<Vec<RequeteHttp>>,
}

impl TransportSimule {
    pub fn nouveau(reponses: Vec<EchangeSimule>) -> Self {
        Self {
            reponses: Mutex::new(reponses.into_iter().collect()),
            requetes: Mutex::new(Vec::new()),
        }
    }

    pub fn requetes(&self) -> Vec<RequeteHttp> {
        self.requetes.lock().expect("registre requêtes").clone()
    }
}

#[async_trait]
impl TransportHttp for TransportSimule {
    async fn envoyer(&self, requete: RequeteHttp) -> Result<ReponseHttp, TransportError> {
        self.requetes
            .lock()
            .expect("registre requêtes")
            .push(requete);
        let echange = self
            .reponses
            .lock()
            .expect("file de réponses")
            .pop_front()
            .expect("réponse simulée disponible");
        Ok(ReponseHttp {
            statut: echange.statut,
            corps: echange.corps,
        })
    }
}
