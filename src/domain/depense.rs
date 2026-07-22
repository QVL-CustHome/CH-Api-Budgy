use crate::domain::category::Category;
use chrono::NaiveDate;
use std::fmt;

#[derive(Debug, thiserror::Error)]
pub enum MoisInvalide {
    #[error("format de mois invalide (YYYY-MM attendu)")]
    Format,
    #[error("mois hors plage (01 à 12 attendu)")]
    HorsPlage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mois {
    annee: i32,
    mois: u32,
}

impl Mois {
    pub fn parse(valeur: &str) -> Result<Self, MoisInvalide> {
        let (annee, mois) = valeur.split_once('-').ok_or(MoisInvalide::Format)?;
        if annee.len() != 4 || mois.len() != 2 {
            return Err(MoisInvalide::Format);
        }
        let annee = annee.parse::<i32>().map_err(|_| MoisInvalide::Format)?;
        let mois = mois.parse::<u32>().map_err(|_| MoisInvalide::Format)?;
        if !(1..=12).contains(&mois) {
            return Err(MoisInvalide::HorsPlage);
        }
        Ok(Self { annee, mois })
    }

    pub fn premier_jour(&self) -> NaiveDate {
        NaiveDate::from_ymd_opt(self.annee, self.mois, 1).expect("mois validé entre 1 et 12")
    }

    pub fn premier_jour_mois_suivant(&self) -> NaiveDate {
        let (annee, mois) = if self.mois == 12 {
            (self.annee + 1, 1)
        } else {
            (self.annee, self.mois + 1)
        };
        NaiveDate::from_ymd_opt(annee, mois, 1).expect("premier jour du mois suivant valide")
    }
}

impl fmt::Display for Mois {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.annee, self.mois)
    }
}

#[derive(Debug, Clone)]
pub struct LigneDepenseCategorie {
    pub category: Option<Category>,
    pub montant_cents: i64,
}

#[derive(Debug, Clone)]
pub struct RepartitionDepenses {
    pub total_cents: i64,
    pub lignes: Vec<LigneDepenseCategorie>,
}
