use crate::adapters::bank::determinisme::{horodatage_ancre, uuid_depuis};
use crate::domain::balance::{Balance, BalanceId, BalanceType};
use crate::domain::bank_account::{BankAccount, BankAccountId};
use crate::domain::compte::ProprietaireId;
use crate::domain::consent::{Consent, ConsentId, ConsentStatus};
use crate::domain::horloge::{Horloge, HorlogeSysteme};
use crate::domain::ports::bank_data_source::{
    BankDataSource, BankDataSourceError, ConsentementInitie, DemandeConsentement, Etablissement,
    ReponseAutorisation,
};
use crate::domain::transaction_bancaire::{
    TransactionBancaire, TransactionBancaireId, TransactionStatus,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration, NaiveDate, Utc};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const DEVISE: &str = "EUR";
const DUREE_CONSENTEMENT_JOURS: i64 = 90;

pub struct MockBankDataSource {
    rejeux_par_compte: Mutex<HashMap<String, u32>>,
    horloge: Arc<dyn Horloge>,
}

impl Default for MockBankDataSource {
    fn default() -> Self {
        Self::avec_horloge(Arc::new(HorlogeSysteme))
    }
}

impl MockBankDataSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn avec_horloge(horloge: Arc<dyn Horloge>) -> Self {
        Self {
            rejeux_par_compte: Mutex::new(HashMap::new()),
            horloge,
        }
    }

    fn expiration_consentement(&self) -> DateTime<Utc> {
        self.horloge.maintenant() + Duration::days(DUREE_CONSENTEMENT_JOURS)
    }

    fn lot_transactions(compte: &BankAccount, rejeu: u32) -> Vec<TransactionBancaire> {
        let cree_le = horodatage_ancre();
        let booking = horodatage_ancre().date_naive();

        let salaire = TransactionBancaire {
            id: TransactionBancaireId(uuid_depuis(&format!("{}-tx-salaire", compte.id.0))),
            bank_account: compte.id.clone(),
            external_transaction_id: format!("{}-salaire", compte.external_account_id),
            status: TransactionStatus::Booked,
            label: "VIREMENT SALAIRE".to_string(),
            amount_cents: 245_000,
            currency: DEVISE.to_string(),
            booking_date: Some(booking - Duration::days(3)),
            value_date: Some(booking - Duration::days(3)),
            created_at: cree_le,
        };

        let abonnement = TransactionBancaire {
            id: TransactionBancaireId(uuid_depuis(&format!("{}-tx-abonnement", compte.id.0))),
            bank_account: compte.id.clone(),
            external_transaction_id: format!("{}-abonnement", compte.external_account_id),
            status: TransactionStatus::Booked,
            label: "PRELEVEMENT ABONNEMENT".to_string(),
            amount_cents: -1_299,
            currency: DEVISE.to_string(),
            booking_date: Some(booking - Duration::days(1)),
            value_date: Some(booking - Duration::days(1)),
            created_at: cree_le,
        };

        let statut_evolutif = if rejeu == 0 {
            TransactionStatus::Pending
        } else {
            TransactionStatus::Booked
        };
        let date_evolutive = if rejeu == 0 { None } else { Some(booking) };

        let achat = TransactionBancaire {
            id: TransactionBancaireId(uuid_depuis(&format!("{}-tx-achat", compte.id.0))),
            bank_account: compte.id.clone(),
            external_transaction_id: format!("{}-achat", compte.external_account_id),
            status: statut_evolutif,
            label: "CARTE ACHAT COMMERCE".to_string(),
            amount_cents: -4_590,
            currency: DEVISE.to_string(),
            booking_date: date_evolutive,
            value_date: date_evolutive,
            created_at: cree_le,
        };

        vec![salaire, abonnement, achat]
    }
}

#[async_trait]
impl BankDataSource for MockBankDataSource {
    async fn lister_etablissements(&self) -> Result<Vec<Etablissement>, BankDataSourceError> {
        Ok(vec![
            Etablissement {
                id: "mock-banque-demo|FR".to_string(),
                nom: "Banque Démo".to_string(),
                pays: "FR".to_string(),
            },
            Etablissement {
                id: "mock-banque-epargne|FR".to_string(),
                nom: "Banque Épargne".to_string(),
                pays: "FR".to_string(),
            },
        ])
    }

    async fn initier_consentement(
        &self,
        demande: DemandeConsentement,
    ) -> Result<ConsentementInitie, BankDataSourceError> {
        let horodatage = horodatage_ancre();
        let consent = Consent {
            id: demande.consent_id,
            proprietaire: demande.proprietaire,
            external_ref: format!("mock-auth-{}", demande.etablissement),
            etablissement: Some(demande.etablissement.clone()),
            status: ConsentStatus::Pending,
            expires_at: Some(self.expiration_consentement()),
            created_at: horodatage,
            updated_at: horodatage,
        };
        let url_autorisation = format!(
            "https://mock.banque.example/authorize?redirect={}&state={}",
            demande.url_retour, consent.id.0
        );
        Ok(ConsentementInitie {
            consent,
            url_autorisation,
        })
    }

    async fn completer_consentement(
        &self,
        proprietaire: &ProprietaireId,
        reponse: ReponseAutorisation,
    ) -> Result<Consent, BankDataSourceError> {
        let horodatage = horodatage_ancre();
        let consent_id = uuid::Uuid::parse_str(&reponse.reference_autorisation)
            .map(ConsentId)
            .map_err(|_| BankDataSourceError::ConsentementInvalide)?;
        Ok(Consent {
            id: consent_id,
            proprietaire: proprietaire.clone(),
            external_ref: format!("mock-session-{}", reponse.reference_autorisation),
            etablissement: None,
            status: ConsentStatus::Active,
            expires_at: Some(self.expiration_consentement()),
            created_at: horodatage,
            updated_at: horodatage,
        })
    }

    async fn lister_comptes(
        &self,
        consent: &Consent,
    ) -> Result<Vec<BankAccount>, BankDataSourceError> {
        let horodatage = horodatage_ancre();
        let proprietaire = consent.proprietaire.clone();
        let comptes = ["courant", "epargne"]
            .into_iter()
            .map(|nature| {
                let external = format!("mock-{}-{nature}", consent.external_ref);
                BankAccount {
                    id: BankAccountId(uuid_depuis(&format!("account-{external}"))),
                    proprietaire: proprietaire.clone(),
                    consent: consent.id.clone(),
                    external_account_id: external,
                    iban_masked: "************0189".to_string(),
                    currency: DEVISE.to_string(),
                    next_sync_at: Some(horodatage + Duration::days(1)),
                    sync_count_today: 0,
                    created_at: horodatage,
                    updated_at: horodatage,
                }
            })
            .collect();
        Ok(comptes)
    }

    async fn solde(
        &self,
        _consent: &Consent,
        compte: &BankAccount,
    ) -> Result<Vec<Balance>, BankDataSourceError> {
        let horodatage = horodatage_ancre();
        Ok(vec![
            Balance {
                id: BalanceId(uuid_depuis(&format!("balance-booked-{}", compte.id.0))),
                bank_account: compte.id.clone(),
                balance_type: BalanceType::Booked,
                amount_cents: 312_711,
                currency: DEVISE.to_string(),
                reference_date: horodatage,
                created_at: horodatage,
            },
            Balance {
                id: BalanceId(uuid_depuis(&format!("balance-available-{}", compte.id.0))),
                bank_account: compte.id.clone(),
                balance_type: BalanceType::Available,
                amount_cents: 308_121,
                currency: DEVISE.to_string(),
                reference_date: horodatage,
                created_at: horodatage,
            },
        ])
    }

    async fn lister_transactions(
        &self,
        _consent: &Consent,
        compte: &BankAccount,
        _depuis: NaiveDate,
    ) -> Result<Vec<TransactionBancaire>, BankDataSourceError> {
        let rejeu = {
            let mut compteurs = self
                .rejeux_par_compte
                .lock()
                .map_err(|_| BankDataSourceError::Technique("état du mock corrompu".to_string()))?;
            let cle = compte.external_account_id.clone();
            let courant = compteurs.entry(cle).or_insert(0);
            let valeur = *courant;
            *courant += 1;
            valeur
        };
        Ok(Self::lot_transactions(compte, rejeu))
    }

    async fn revoquer_consentement(
        &self,
        consent: &Consent,
    ) -> Result<Consent, BankDataSourceError> {
        Ok(Consent {
            status: ConsentStatus::Revoked,
            updated_at: horodatage_ancre(),
            ..consent.clone()
        })
    }
}
