use crate::adapters::bank::enable_banking::ClientEnableBanking;
use crate::adapters::bank::enable_banking::jwt::SignataireJwt;
use crate::adapters::bank::enable_banking::transport::{ReqwestTransport, TransportHttp};
use crate::config::EnableBankingConfig;
use crate::domain::balance::Balance;
use crate::domain::bank_account::BankAccount;
use crate::domain::compte::ProprietaireId;
use crate::domain::consent::Consent;
use crate::domain::ports::bank_data_source::{
    BankDataSource, BankDataSourceError, ConsentementInitie, DemandeConsentement, Etablissement,
    ReponseAutorisation,
};
use crate::domain::transaction_bancaire::TransactionBancaire;
use async_trait::async_trait;
use chrono::{NaiveDate, Utc};

pub struct EnableBankingBankDataSource<T: TransportHttp = ReqwestTransport> {
    client: Option<ClientEnableBanking<T>>,
}

impl EnableBankingBankDataSource<ReqwestTransport> {
    pub fn depuis_config(config: &EnableBankingConfig) -> Self {
        Self {
            client: construire_client(config),
        }
    }
}

impl<T: TransportHttp> EnableBankingBankDataSource<T> {
    pub fn avec_client(client: ClientEnableBanking<T>) -> Self {
        Self {
            client: Some(client),
        }
    }

    fn client(&self) -> Result<&ClientEnableBanking<T>, BankDataSourceError> {
        self.client
            .as_ref()
            .ok_or(BankDataSourceError::SourceNonConfiguree)
    }
}

fn construire_client(
    config: &EnableBankingConfig,
) -> Option<ClientEnableBanking<ReqwestTransport>> {
    let app_id = config.app_id.as_ref()?;
    let redirect_url = config.redirect_url.as_ref()?;
    let pem = charger_pem(config)?;
    let signataire = SignataireJwt::nouveau(app_id, &pem).ok()?;
    let transport = ReqwestTransport::nouveau(&config.base_url);
    Some(ClientEnableBanking::nouveau(
        transport,
        signataire,
        redirect_url.clone(),
    ))
}

fn charger_pem(config: &EnableBankingConfig) -> Option<Vec<u8>> {
    if let Some(pem) = &config.private_key_pem {
        return Some(pem.clone().into_bytes());
    }
    let chemin = config.private_key_path.as_ref()?;
    std::fs::read(chemin).ok()
}

#[async_trait]
impl<T: TransportHttp> BankDataSource for EnableBankingBankDataSource<T> {
    async fn lister_etablissements(&self) -> Result<Vec<Etablissement>, BankDataSourceError> {
        self.client()?.lister_etablissements().await
    }

    async fn initier_consentement(
        &self,
        demande: DemandeConsentement,
    ) -> Result<ConsentementInitie, BankDataSourceError> {
        self.client()?
            .initier_consentement(demande, Utc::now())
            .await
    }

    async fn completer_consentement(
        &self,
        proprietaire: &ProprietaireId,
        reponse: ReponseAutorisation,
    ) -> Result<Consent, BankDataSourceError> {
        self.client()?
            .completer_consentement(proprietaire, reponse, Utc::now())
            .await
    }

    async fn lister_comptes(
        &self,
        consent: &Consent,
    ) -> Result<Vec<BankAccount>, BankDataSourceError> {
        self.client()?.lister_comptes(consent, Utc::now()).await
    }

    async fn solde(
        &self,
        _consent: &Consent,
        compte: &BankAccount,
    ) -> Result<Vec<Balance>, BankDataSourceError> {
        self.client()?.solde(compte, Utc::now()).await
    }

    async fn lister_transactions(
        &self,
        _consent: &Consent,
        compte: &BankAccount,
        depuis: NaiveDate,
    ) -> Result<Vec<TransactionBancaire>, BankDataSourceError> {
        self.client()?
            .lister_transactions(compte, depuis, Utc::now())
            .await
    }

    async fn revoquer_consentement(
        &self,
        consent: &Consent,
    ) -> Result<Consent, BankDataSourceError> {
        self.client()?
            .revoquer_consentement(consent, Utc::now())
            .await
    }
}
