use crate::domain::compte::ProprietaireId;
use crate::domain::ports::bank_data_source::BankDataSource;
use crate::domain::ports::ecriture::{
    BankAccountsWriteRepository, ConsentsWriteRepository, EcritureError,
};
use crate::domain::ports::lecture::{ConsentsReadRepository, LectureError};

#[derive(Debug, thiserror::Error)]
pub enum EffacementError {
    #[error("lecture des consentements impossible : {0}")]
    Lecture(#[from] LectureError),
    #[error("suppression des données impossible : {0}")]
    Ecriture(#[from] EcritureError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RapportEffacement {
    pub consentements_supprimes: u64,
    pub comptes_supprimes: u64,
    pub revocations_demandees: u64,
    pub revocations_echouees: u64,
}

pub struct EffacementProprietaire<L, C, B, S>
where
    L: ConsentsReadRepository,
    C: ConsentsWriteRepository,
    B: BankAccountsWriteRepository,
    S: BankDataSource + ?Sized,
{
    consents_lecture: L,
    consents_ecriture: C,
    comptes_ecriture: B,
    source_bancaire: std::sync::Arc<S>,
}

impl<L, C, B, S> EffacementProprietaire<L, C, B, S>
where
    L: ConsentsReadRepository,
    C: ConsentsWriteRepository,
    B: BankAccountsWriteRepository,
    S: BankDataSource + ?Sized,
{
    pub fn new(
        consents_lecture: L,
        consents_ecriture: C,
        comptes_ecriture: B,
        source_bancaire: std::sync::Arc<S>,
    ) -> Self {
        Self {
            consents_lecture,
            consents_ecriture,
            comptes_ecriture,
            source_bancaire,
        }
    }

    pub async fn effacer_donnees_proprietaire(
        &self,
        proprietaire: ProprietaireId,
    ) -> Result<RapportEffacement, EffacementError> {
        let mut rapport = RapportEffacement::default();

        let actifs = self
            .consents_lecture
            .lister_actifs_par_proprietaire(&proprietaire)
            .await?;

        for consent in &actifs {
            rapport.revocations_demandees += 1;
            if let Err(erreur) = self.source_bancaire.revoquer_consentement(consent).await {
                rapport.revocations_echouees += 1;
                tracing::warn!(
                    consent_id = %consent.id.0,
                    cause = %erreur,
                    "révocation du consentement côté fournisseur en échec, effacement local poursuivi"
                );
            }
        }

        rapport.comptes_supprimes = self
            .comptes_ecriture
            .supprimer_par_proprietaire(&proprietaire)
            .await?;

        rapport.consentements_supprimes = self
            .consents_ecriture
            .supprimer_par_proprietaire(&proprietaire)
            .await?;

        Ok(rapport)
    }
}
