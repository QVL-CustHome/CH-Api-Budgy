#![allow(dead_code)]

use async_trait::async_trait;
use ch_api_budgy::adapters::bank::enable_banking::transport::{
    ReponseHttp, RequeteHttp, TransportError, TransportHttp,
};
use ch_api_budgy::domain::balance::{Balance, BalanceId, BalanceType, NouvelleBalance};
use ch_api_budgy::domain::bank_account::BankAccount;
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::{Consent, ConsentId, ConsentStatus};
use ch_api_budgy::domain::ports::bank_data_source::{
    BankDataSource, BankDataSourceError, ConsentementInitie, DemandeConsentement, Etablissement,
    ReponseAutorisation,
};
use ch_api_budgy::domain::ports::ecriture::{
    BalancesWriteRepository, BankTransactionsWriteRepository, ConsentsStatutWriteRepository,
    EcritureError, ResultatInsertion,
};
use ch_api_budgy::domain::transaction_bancaire::{
    CategorizationSource, NouvelleTransactionBancaire, TransactionBancaire, TransactionBancaireId,
    TransactionStatus,
};
use chrono::{NaiveDate, TimeZone, Utc};
use rsa::RsaPrivateKey;
use rsa::pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey, LineEnding};
use std::collections::VecDeque;
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct BalancesMemoireStub;

impl BalancesWriteRepository for BalancesMemoireStub {
    async fn enregistrer(&self, _nouvelle: NouvelleBalance) -> Result<BalanceId, EcritureError> {
        Ok(BalanceId(Uuid::new_v4()))
    }
}

#[derive(Clone, Default)]
pub struct TransactionsMemoireStub;

impl BankTransactionsWriteRepository for TransactionsMemoireStub {
    async fn enregistrer(
        &self,
        _nouvelle: NouvelleTransactionBancaire,
    ) -> Result<ResultatInsertion<TransactionBancaireId>, EcritureError> {
        Ok(ResultatInsertion::Inseree(TransactionBancaireId(
            Uuid::new_v4(),
        )))
    }
}

#[derive(Clone, Default)]
pub struct ConsentsStatutStub;

impl ConsentsStatutWriteRepository for ConsentsStatutStub {
    async fn marquer_statut(
        &self,
        _consent: &ConsentId,
        _statut: ConsentStatus,
    ) -> Result<(), EcritureError> {
        Ok(())
    }
}

pub struct SourceBancaireFake {
    en_echec: bool,
}

impl SourceBancaireFake {
    pub fn operationnelle() -> Self {
        Self { en_echec: false }
    }

    pub fn en_echec() -> Self {
        Self { en_echec: true }
    }
}

#[async_trait]
impl BankDataSource for SourceBancaireFake {
    async fn lister_etablissements(&self) -> Result<Vec<Etablissement>, BankDataSourceError> {
        Ok(Vec::new())
    }

    async fn initier_consentement(
        &self,
        _demande: DemandeConsentement,
    ) -> Result<ConsentementInitie, BankDataSourceError> {
        Err(BankDataSourceError::SourceNonConfiguree)
    }

    async fn completer_consentement(
        &self,
        _proprietaire: &ProprietaireId,
        _reponse: ReponseAutorisation,
    ) -> Result<Consent, BankDataSourceError> {
        Err(BankDataSourceError::SourceNonConfiguree)
    }

    async fn lister_comptes(
        &self,
        _consent: &Consent,
    ) -> Result<Vec<BankAccount>, BankDataSourceError> {
        Ok(Vec::new())
    }

    async fn solde(
        &self,
        _consent: &Consent,
        compte: &BankAccount,
    ) -> Result<Vec<Balance>, BankDataSourceError> {
        if self.en_echec {
            return Err(BankDataSourceError::ConsentementInvalide);
        }
        Ok(vec![Balance {
            id: BalanceId(Uuid::new_v4()),
            bank_account: compte.id.clone(),
            balance_type: BalanceType::Available,
            amount_cents: 100_000,
            currency: compte.currency.clone(),
            reference_date: Utc.with_ymd_and_hms(2026, 6, 27, 0, 0, 0).unwrap(),
            created_at: Utc.with_ymd_and_hms(2026, 6, 27, 0, 0, 0).unwrap(),
        }])
    }

    async fn lister_transactions(
        &self,
        _consent: &Consent,
        compte: &BankAccount,
        _depuis: NaiveDate,
    ) -> Result<Vec<TransactionBancaire>, BankDataSourceError> {
        Ok(vec![TransactionBancaire {
            id: TransactionBancaireId(Uuid::new_v4()),
            bank_account: compte.id.clone(),
            external_transaction_id: "tx-1".to_string(),
            status: TransactionStatus::Booked,
            label: "ACHAT".to_string(),
            amount_cents: -1_299,
            currency: "EUR".to_string(),
            booking_date: None,
            value_date: None,
            category: None,
            categorization_source: CategorizationSource::None,
            rule_id: None,
            created_at: Utc.with_ymd_and_hms(2026, 6, 27, 0, 0, 0).unwrap(),
        }])
    }

    async fn revoquer_consentement(
        &self,
        consent: &Consent,
    ) -> Result<Consent, BankDataSourceError> {
        Ok(consent.clone())
    }
}

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
