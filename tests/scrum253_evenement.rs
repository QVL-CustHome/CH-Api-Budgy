use async_trait::async_trait;
use ch_api_budgy::domain::balance::Balance;
use ch_api_budgy::domain::bank_account::{BankAccount, BankAccountId, NouveauBankAccount};
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::{
    Consent, ConsentId, ConsentStatus, MiseAJourConsent, NouveauConsent, NouveauConsentInitie,
};
use ch_api_budgy::domain::effacement::EffacementProprietaire;
use ch_api_budgy::domain::ports::bank_data_source::{
    BankDataSource, BankDataSourceError, ConsentementInitie, DemandeConsentement, Etablissement,
    ReponseAutorisation,
};
use ch_api_budgy::domain::ports::ecriture::{
    BankAccountsWriteRepository, ConsentsWriteRepository, EcritureError,
};
use ch_api_budgy::domain::ports::lecture::{ConsentsReadRepository, LectureError};
use ch_api_budgy::domain::transaction_bancaire::TransactionBancaire;
use ch_api_budgy::relay::evenement::parser_user_deleted;
use chrono::{NaiveDate, Utc};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

const SUB: &str = "65a1f3c2d4e5f6a7b8c9d0e1";

fn consent_actif(owner: &str) -> Consent {
    let maintenant = Utc::now();
    Consent {
        id: ConsentId(Uuid::new_v4()),
        proprietaire: ProprietaireId(owner.to_string()),
        external_ref: "session-ref".to_string(),
        etablissement: None,
        status: ConsentStatus::Active,
        expires_at: None,
        created_at: maintenant,
        updated_at: maintenant,
    }
}

#[derive(Clone, Default)]
struct ConsentsEnMemoire {
    actifs: Arc<Mutex<Vec<Consent>>>,
    supprimes: Arc<AtomicU64>,
}

impl ConsentsReadRepository for ConsentsEnMemoire {
    async fn lister_actifs_par_proprietaire(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<Vec<Consent>, LectureError> {
        let actifs = self.actifs.lock().expect("verrou actifs");
        Ok(actifs
            .iter()
            .filter(|c| &c.proprietaire == proprietaire)
            .cloned()
            .collect())
    }

    async fn lister_par_proprietaire(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<Vec<Consent>, LectureError> {
        self.lister_actifs_par_proprietaire(proprietaire).await
    }

    async fn fetch_pour_proprietaire(
        &self,
        proprietaire: &ProprietaireId,
        id: &ConsentId,
    ) -> Result<Option<Consent>, LectureError> {
        let actifs = self.actifs.lock().expect("verrou actifs");
        Ok(actifs
            .iter()
            .find(|c| &c.proprietaire == proprietaire && &c.id == id)
            .cloned())
    }
}

impl ConsentsWriteRepository for ConsentsEnMemoire {
    async fn enregistrer(&self, _nouveau: NouveauConsent) -> Result<ConsentId, EcritureError> {
        Ok(ConsentId(Uuid::new_v4()))
    }

    async fn enregistrer_initie(
        &self,
        nouveau: NouveauConsentInitie,
    ) -> Result<ConsentId, EcritureError> {
        Ok(nouveau.id)
    }

    async fn mettre_a_jour(
        &self,
        _proprietaire: &ProprietaireId,
        _id: &ConsentId,
        _mise_a_jour: MiseAJourConsent,
    ) -> Result<bool, EcritureError> {
        Ok(true)
    }

    async fn marquer_statut(
        &self,
        _proprietaire: &ProprietaireId,
        _id: &ConsentId,
        _status: ConsentStatus,
    ) -> Result<bool, EcritureError> {
        Ok(true)
    }

    async fn supprimer_par_proprietaire(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<u64, EcritureError> {
        let mut actifs = self.actifs.lock().expect("verrou actifs");
        let avant = actifs.len() as u64;
        actifs.retain(|c| &c.proprietaire != proprietaire);
        let supprimes = avant - actifs.len() as u64;
        self.supprimes.fetch_add(supprimes, Ordering::SeqCst);
        Ok(supprimes)
    }
}

#[derive(Clone, Default)]
struct ComptesEnMemoire {
    supprimes: Arc<AtomicU64>,
}

impl BankAccountsWriteRepository for ComptesEnMemoire {
    async fn enregistrer(
        &self,
        _nouveau: NouveauBankAccount,
    ) -> Result<BankAccountId, EcritureError> {
        Ok(BankAccountId(Uuid::new_v4()))
    }

    async fn supprimer_par_proprietaire(
        &self,
        _proprietaire: &ProprietaireId,
    ) -> Result<u64, EcritureError> {
        self.supprimes.fetch_add(1, Ordering::SeqCst);
        Ok(1)
    }
}

struct SourceRevocation {
    revocations: AtomicU64,
    echoue: bool,
}

impl SourceRevocation {
    fn nouvelle(echoue: bool) -> Self {
        Self {
            revocations: AtomicU64::new(0),
            echoue,
        }
    }
}

#[async_trait]
impl BankDataSource for SourceRevocation {
    async fn lister_etablissements(&self) -> Result<Vec<Etablissement>, BankDataSourceError> {
        Ok(vec![])
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
        Ok(vec![])
    }

    async fn solde(
        &self,
        _consent: &Consent,
        _compte: &BankAccount,
    ) -> Result<Vec<Balance>, BankDataSourceError> {
        Ok(vec![])
    }

    async fn lister_transactions(
        &self,
        _consent: &Consent,
        _compte: &BankAccount,
        _depuis: NaiveDate,
    ) -> Result<Vec<TransactionBancaire>, BankDataSourceError> {
        Ok(vec![])
    }

    async fn revoquer_consentement(
        &self,
        consent: &Consent,
    ) -> Result<Consent, BankDataSourceError> {
        self.revocations.fetch_add(1, Ordering::SeqCst);
        if self.echoue {
            return Err(BankDataSourceError::EtablissementIndisponible);
        }
        Ok(Consent {
            status: ConsentStatus::Revoked,
            ..consent.clone()
        })
    }
}

#[test]
fn parsing_payload_valide_extrait_le_sub() {
    let payload = format!(
        "{{\"event_id\":\"{}\",\"event_type\":\"auth.user.deleted\",\"sub\":\"{SUB}\",\"occurred_at\":\"2026-06-25T10:00:00Z\"}}",
        Uuid::new_v4()
    );
    let proprietaire = parser_user_deleted(payload.as_bytes()).expect("payload valide");
    assert_eq!(proprietaire, ProprietaireId(SUB.to_string()));
}

#[test]
fn parsing_type_inattendu_rejete() {
    let payload = r#"{"event_type":"auth.user.updated","sub":"x"}"#;
    assert!(parser_user_deleted(payload.as_bytes()).is_err());
}

#[test]
fn parsing_sub_manquant_rejete() {
    let payload = r#"{"event_type":"auth.user.deleted","sub":"   "}"#;
    assert!(parser_user_deleted(payload.as_bytes()).is_err());
}

#[test]
fn parsing_json_corrompu_ne_panique_pas() {
    assert!(parser_user_deleted(b"pas du json").is_err());
    assert!(parser_user_deleted(b"").is_err());
}

#[tokio::test]
async fn revocation_appelee_pour_chaque_consentement_actif() {
    let consents = ConsentsEnMemoire::default();
    {
        let mut actifs = consents.actifs.lock().expect("verrou actifs");
        actifs.push(consent_actif(SUB));
        actifs.push(consent_actif(SUB));
    }
    let comptes = ComptesEnMemoire::default();
    let source = Arc::new(SourceRevocation::nouvelle(false));

    let service =
        EffacementProprietaire::new(consents.clone(), consents.clone(), comptes, source.clone());
    let rapport = service
        .effacer_donnees_proprietaire(ProprietaireId(SUB.to_string()))
        .await
        .expect("effacement réussi");

    assert_eq!(source.revocations.load(Ordering::SeqCst), 2);
    assert_eq!(rapport.revocations_demandees, 2);
    assert_eq!(rapport.consentements_supprimes, 2);
}

#[tokio::test]
async fn suppression_locale_aboutit_meme_si_revocation_fournisseur_echoue() {
    let consents = ConsentsEnMemoire::default();
    {
        let mut actifs = consents.actifs.lock().expect("verrou actifs");
        actifs.push(consent_actif(SUB));
    }
    let comptes = ComptesEnMemoire::default();
    let source = Arc::new(SourceRevocation::nouvelle(true));

    let service = EffacementProprietaire::new(
        consents.clone(),
        consents.clone(),
        comptes.clone(),
        source.clone(),
    );
    let rapport = service
        .effacer_donnees_proprietaire(ProprietaireId(SUB.to_string()))
        .await
        .expect("effacement local aboutit malgré l'échec fournisseur");

    assert_eq!(rapport.revocations_echouees, 1);
    assert_eq!(rapport.consentements_supprimes, 1);
    assert_eq!(comptes.supprimes.load(Ordering::SeqCst), 1);
    assert!(consents.actifs.lock().expect("verrou actifs").is_empty());
}
