pub mod dto;
pub mod jwt;
pub mod mapping;
pub mod transport;

use crate::adapters::bank::enable_banking::dto::{
    AccessDemande, AspspReference, DemandeAuth, DemandeSession, ReponseAspsps, ReponseAuth,
    ReponseBalances, ReponseSession, ReponseTransactions,
};
use crate::adapters::bank::enable_banking::jwt::SignataireJwt;
use crate::adapters::bank::enable_banking::mapping::{
    consent_id_depuis_reference, vers_balance, vers_bank_account, vers_etablissement,
    vers_transaction,
};
use crate::adapters::bank::enable_banking::transport::{
    MethodeHttp, ReponseHttp, RequeteHttp, TransportError, TransportHttp,
};
use crate::domain::balance::Balance;
use crate::domain::bank_account::BankAccount;
use crate::domain::compte::ProprietaireId;
use crate::domain::consent::{Consent, ConsentStatus};
use crate::domain::ports::bank_data_source::{
    BankDataSourceError, ConsentementInitie, DemandeConsentement, Etablissement,
    ReponseAutorisation,
};
use crate::domain::transaction_bancaire::TransactionBancaire;
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::de::DeserializeOwned;

const PSU_TYPE_PERSONNEL: &str = "personal";
const VALIDITE_CONSENTEMENT_JOURS: i64 = 90;
const PAYS_DEFAUT: &str = "FR";

pub struct ClientEnableBanking<T: TransportHttp> {
    transport: T,
    signataire: SignataireJwt,
    redirect_url: String,
}

impl<T: TransportHttp> ClientEnableBanking<T> {
    pub fn nouveau(transport: T, signataire: SignataireJwt, redirect_url: String) -> Self {
        Self {
            transport,
            signataire,
            redirect_url,
        }
    }

    async fn appeler(
        &self,
        methode: MethodeHttp,
        chemin: String,
        corps_json: Option<String>,
    ) -> Result<ReponseHttp, BankDataSourceError> {
        let jeton = self
            .signataire
            .jeton()
            .map_err(|e| BankDataSourceError::Technique(e.to_string()))?;
        let reponse = self
            .transport
            .envoyer(RequeteHttp {
                methode,
                chemin,
                jeton,
                corps_json,
            })
            .await
            .map_err(traduire_transport)?;
        verifier_statut(&reponse)?;
        Ok(reponse)
    }

    fn deserialiser<R: DeserializeOwned>(corps: &str) -> Result<R, BankDataSourceError> {
        serde_json::from_str(corps).map_err(|e| BankDataSourceError::ReponseInvalide(e.to_string()))
    }

    fn url_retour(&self, demande: &DemandeConsentement) -> String {
        let demandee = demande.url_retour.trim();
        if demandee.is_empty() {
            self.redirect_url.clone()
        } else {
            demandee.to_string()
        }
    }

    pub async fn lister_etablissements(&self) -> Result<Vec<Etablissement>, BankDataSourceError> {
        let reponse = self
            .appeler(MethodeHttp::Get, "/aspsps".to_string(), None)
            .await?;
        let aspsps: ReponseAspsps = Self::deserialiser(&reponse.corps)?;
        // Restriction volontaire des banques proposees : uniquement les caisses
        // Credit Agricole (toutes contiennent "agricole", insensible aux accents)
        // et Boursorama ("bourso"). Evite de noyer l'UI sous les 126 ASPSP FR.
        Ok(aspsps
            .aspsps
            .iter()
            .filter(|a| {
                let nom = a.name.to_lowercase();
                nom.contains("agricole") || nom.contains("bourso")
            })
            .map(vers_etablissement)
            .collect())
    }

    pub async fn initier_consentement(
        &self,
        demande: DemandeConsentement,
        horodatage: DateTime<Utc>,
    ) -> Result<ConsentementInitie, BankDataSourceError> {
        let valid_until = (horodatage + Duration::days(VALIDITE_CONSENTEMENT_JOURS)).to_rfc3339();
        let state = demande.consent_id.0.to_string();
        // L'identifiant d'etablissement expose a l'UI a le format "nom|pays"
        // (cf. vers_etablissement). EnableBanking /auth attend le NOM exact de
        // l'ASPSP et son pays separement -> on re-decoupe sur le dernier '|'
        // (sinon le suffixe "|FR" fait echouer /auth en 422 WRONG_ASPSP).
        let (aspsp_name, aspsp_country) = match demande.etablissement.rsplit_once('|') {
            Some((nom, pays)) if !pays.is_empty() => (nom.to_string(), pays.to_string()),
            _ => (demande.etablissement.clone(), PAYS_DEFAUT.to_string()),
        };
        let corps = DemandeAuth {
            access: AccessDemande { valid_until },
            aspsp: AspspReference {
                name: aspsp_name,
                country: aspsp_country,
            },
            state: state.clone(),
            redirect_url: self.url_retour(&demande),
            psu_type: PSU_TYPE_PERSONNEL.to_string(),
        };
        let corps_json = serde_json::to_string(&corps)
            .map_err(|e| BankDataSourceError::Technique(e.to_string()))?;
        let reponse = self
            .appeler(MethodeHttp::Post, "/auth".to_string(), Some(corps_json))
            .await?;
        let auth: ReponseAuth = Self::deserialiser(&reponse.corps)?;

        let consent = Consent {
            id: demande.consent_id,
            proprietaire: demande.proprietaire,
            external_ref: auth.authorization_id,
            etablissement: Some(demande.etablissement),
            status: ConsentStatus::Pending,
            expires_at: Some(horodatage + Duration::days(VALIDITE_CONSENTEMENT_JOURS)),
            created_at: horodatage,
            updated_at: horodatage,
        };
        Ok(ConsentementInitie {
            consent,
            url_autorisation: ajouter_state(&auth.url, &state),
        })
    }

    pub async fn completer_consentement(
        &self,
        proprietaire: &ProprietaireId,
        reponse: ReponseAutorisation,
        horodatage: DateTime<Utc>,
    ) -> Result<Consent, BankDataSourceError> {
        let consent_id = consent_id_depuis_reference(&reponse.reference_autorisation)
            .ok_or(BankDataSourceError::ConsentementInvalide)?;
        let corps = DemandeSession {
            code: reponse.code_autorisation,
        };
        let corps_json = serde_json::to_string(&corps)
            .map_err(|e| BankDataSourceError::Technique(e.to_string()))?;
        let http = self
            .appeler(MethodeHttp::Post, "/sessions".to_string(), Some(corps_json))
            .await?;
        let session: ReponseSession = Self::deserialiser(&http.corps)?;
        statut_session_exploitable(session.status.as_deref())?;

        let expires_at = session
            .access
            .as_ref()
            .and_then(|a| a.valid_until.as_ref())
            .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or(horodatage + Duration::days(VALIDITE_CONSENTEMENT_JOURS));

        Ok(Consent {
            id: consent_id,
            proprietaire: proprietaire.clone(),
            external_ref: session.session_id,
            etablissement: None,
            status: ConsentStatus::Active,
            expires_at: Some(expires_at),
            created_at: horodatage,
            updated_at: horodatage,
        })
    }

    pub async fn lister_comptes(
        &self,
        consent: &Consent,
        horodatage: DateTime<Utc>,
    ) -> Result<Vec<BankAccount>, BankDataSourceError> {
        let chemin = format!("/sessions/{}", consent.external_ref);
        let http = self.appeler(MethodeHttp::Get, chemin, None).await?;
        let session: ReponseSession = Self::deserialiser(&http.corps)?;
        Ok(session
            .accounts
            .iter()
            .map(|compte| vers_bank_account(compte, consent, horodatage))
            .collect())
    }

    pub async fn solde(
        &self,
        compte: &BankAccount,
        horodatage: DateTime<Utc>,
    ) -> Result<Vec<Balance>, BankDataSourceError> {
        let chemin = format!("/accounts/{}/balances", compte.external_account_id);
        let http = self.appeler(MethodeHttp::Get, chemin, None).await?;
        let balances: ReponseBalances = Self::deserialiser(&http.corps)?;
        balances
            .balances
            .iter()
            .map(|balance| vers_balance(balance, compte, horodatage))
            .collect()
    }

    pub async fn lister_transactions(
        &self,
        compte: &BankAccount,
        depuis: NaiveDate,
        horodatage: DateTime<Utc>,
    ) -> Result<Vec<TransactionBancaire>, BankDataSourceError> {
        let mut transactions = Vec::new();
        let mut continuation: Option<String> = None;

        loop {
            let mut chemin = format!(
                "/accounts/{}/transactions?date_from={}",
                compte.external_account_id,
                depuis.format("%Y-%m-%d")
            );
            if let Some(cle) = &continuation {
                chemin.push_str(&format!("&continuation_key={cle}"));
            }
            let http = self.appeler(MethodeHttp::Get, chemin, None).await?;
            let page: ReponseTransactions = Self::deserialiser(&http.corps)?;

            for transaction in &page.transactions {
                transactions.push(vers_transaction(transaction, compte, horodatage)?);
            }

            match page.continuation_key {
                Some(cle) if !cle.is_empty() => continuation = Some(cle),
                _ => break,
            }
        }
        Ok(transactions)
    }

    pub async fn revoquer_consentement(
        &self,
        consent: &Consent,
        horodatage: DateTime<Utc>,
    ) -> Result<Consent, BankDataSourceError> {
        let chemin = format!("/sessions/{}", consent.external_ref);
        self.appeler(MethodeHttp::Delete, chemin, None).await?;
        Ok(Consent {
            status: ConsentStatus::Revoked,
            updated_at: horodatage,
            ..consent.clone()
        })
    }
}

fn ajouter_state(url: &str, state: &str) -> String {
    if url.contains(&format!("state={state}")) {
        return url.to_string();
    }
    let separateur = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separateur}state={state}")
}

fn traduire_transport(erreur: TransportError) -> BankDataSourceError {
    match erreur {
        TransportError::Reseau(message) => BankDataSourceError::Technique(message),
    }
}

fn verifier_statut(reponse: &ReponseHttp) -> Result<(), BankDataSourceError> {
    if reponse.est_succes() {
        return Ok(());
    }
    match reponse.statut {
        401 | 403 => Err(BankDataSourceError::ConsentementInvalide),
        404 => Err(BankDataSourceError::RessourceIntrouvable),
        429 | 500 | 502 | 503 | 504 => Err(BankDataSourceError::EtablissementIndisponible),
        autre => Err(BankDataSourceError::Technique(format!(
            "statut HTTP inattendu : {autre}"
        ))),
    }
}

const STATUTS_SESSION_REJETES: [&str; 4] = ["REJECTED", "REVOKED", "EXPIRED", "CANCELLED"];

fn statut_session_exploitable(statut: Option<&str>) -> Result<(), BankDataSourceError> {
    match statut {
        Some(valeur) if STATUTS_SESSION_REJETES.contains(&valeur.to_uppercase().as_str()) => {
            Err(BankDataSourceError::ConsentementInvalide)
        }
        _ => Ok(()),
    }
}
