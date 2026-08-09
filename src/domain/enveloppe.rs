//! Enveloppes budgétaires — les « budgets » de l'interface.
//!
//! À distinguer de [`crate::domain::budget`], qui plafonne une catégorie sur un
//! mois donné. Une enveloppe ne dépend d'aucun mois : elle vit jusqu'à sa
//! suppression, et c'est l'utilisateur qui y range ses transactions. Aucune
//! règle ne les affecte automatiquement — c'est voulu : une enveloppe suit un
//! projet (des vacances, un achat), que rien dans le libellé d'une opération ne
//! permet de deviner.

use crate::domain::compte::ProprietaireId;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub const ENVELOPPE_NOM_MAX_LEN: usize = 30;
pub const DEFAULT_ENVELOPPE_COLOR: &str = "#5E35B1";
pub const DEFAULT_ENVELOPPE_ICON: &str = "wallet";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnveloppeId(pub Uuid);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EnveloppeValidationError {
    #[error("le nom du budget est obligatoire")]
    NomVide,
    #[error("le nom du budget ne peut pas dépasser 30 caractères")]
    NomTropLong,
    #[error("le montant du budget ne peut pas être négatif")]
    MontantNegatif,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnveloppeNom(String);

impl EnveloppeNom {
    pub fn parse(valeur: &str) -> Result<Self, EnveloppeValidationError> {
        let nettoye = valeur.trim();
        if nettoye.is_empty() {
            return Err(EnveloppeValidationError::NomVide);
        }
        if nettoye.chars().count() > ENVELOPPE_NOM_MAX_LEN {
            return Err(EnveloppeValidationError::NomTropLong);
        }
        Ok(Self(nettoye.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MontantEnveloppe(i64);

impl MontantEnveloppe {
    pub fn parse(cents: i64) -> Result<Self, EnveloppeValidationError> {
        if cents < 0 {
            return Err(EnveloppeValidationError::MontantNegatif);
        }
        Ok(Self(cents))
    }

    pub fn cents(&self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct Enveloppe {
    pub id: EnveloppeId,
    pub proprietaire: ProprietaireId,
    pub nom: String,
    pub icon: String,
    pub color: String,
    pub montant_cents: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NouvelleEnveloppe {
    pub proprietaire: ProprietaireId,
    pub nom: EnveloppeNom,
    pub icon: String,
    pub color: String,
    pub montant: MontantEnveloppe,
}

#[derive(Debug, Clone)]
pub struct MiseAJourEnveloppe {
    pub nom: EnveloppeNom,
    pub icon: String,
    pub color: String,
    pub montant: MontantEnveloppe,
}

/// Une enveloppe et ce qui y a été dépensé, tous mois confondus.
#[derive(Debug, Clone)]
pub struct SuiviEnveloppe {
    pub enveloppe: Enveloppe,
    pub depense_cents: i64,
    pub nombre_transactions: i64,
}

impl SuiviEnveloppe {
    /// Ce qu'il reste à dépenser. Négatif une fois l'enveloppe dépassée : on
    /// montre le dépassement plutôt que de l'écraser à zéro.
    pub fn restant_cents(&self) -> i64 {
        self.enveloppe.montant_cents - self.depense_cents
    }

    pub fn depasse(&self) -> bool {
        self.restant_cents() < 0
    }

    /// Part consommée, bornée à 100 % pour l'affichage d'une jauge.
    pub fn pourcentage_consomme(&self) -> u8 {
        if self.enveloppe.montant_cents <= 0 {
            return if self.depense_cents > 0 { 100 } else { 0 };
        }
        let ratio = (self.depense_cents as f64 / self.enveloppe.montant_cents as f64) * 100.0;
        ratio.clamp(0.0, 100.0).round() as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suivi(montant_cents: i64, depense_cents: i64) -> SuiviEnveloppe {
        SuiviEnveloppe {
            enveloppe: Enveloppe {
                id: EnveloppeId(Uuid::nil()),
                proprietaire: ProprietaireId("u".to_string()),
                nom: "Vacances".to_string(),
                icon: DEFAULT_ENVELOPPE_ICON.to_string(),
                color: DEFAULT_ENVELOPPE_COLOR.to_string(),
                montant_cents,
                created_at: Utc::now(),
            },
            depense_cents,
            nombre_transactions: 1,
        }
    }

    #[test]
    fn le_restant_devient_negatif_au_dela_du_montant() {
        let depasse = suivi(50_000, 62_500);
        assert_eq!(depasse.restant_cents(), -12_500);
        assert!(depasse.depasse());
    }

    #[test]
    fn la_jauge_se_borne_a_cent_pour_cent() {
        assert_eq!(suivi(50_000, 62_500).pourcentage_consomme(), 100);
        assert_eq!(suivi(50_000, 25_000).pourcentage_consomme(), 50);
        assert_eq!(suivi(50_000, 0).pourcentage_consomme(), 0);
    }

    #[test]
    fn une_enveloppe_a_zero_est_pleine_des_la_premiere_depense() {
        assert_eq!(suivi(0, 0).pourcentage_consomme(), 0);
        assert_eq!(suivi(0, 1).pourcentage_consomme(), 100);
    }

    #[test]
    fn le_nom_est_valide_et_borne() {
        assert_eq!(
            EnveloppeNom::parse("  "),
            Err(EnveloppeValidationError::NomVide)
        );
        assert_eq!(
            EnveloppeNom::parse(&"a".repeat(31)),
            Err(EnveloppeValidationError::NomTropLong)
        );
        assert_eq!(
            EnveloppeNom::parse("  Vacances  ")
                .expect("nom valide")
                .as_str(),
            "Vacances"
        );
    }

    #[test]
    fn le_montant_refuse_le_negatif() {
        assert_eq!(
            MontantEnveloppe::parse(-1),
            Err(EnveloppeValidationError::MontantNegatif)
        );
        assert_eq!(MontantEnveloppe::parse(0).expect("zéro admis").cents(), 0);
    }
}
