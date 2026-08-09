//! Découpage du temps en « mois budgétaires ».
//!
//! Un mois calendaire ne correspond pas toujours au rythme d'un budget : quand
//! le salaire tombe le 28, tout ce qui se passe entre le 28 et la fin du mois
//! appartient déjà au budget suivant. L'utilisateur choisit donc le jour de
//! départ, et tous les calculs mensuels s'appuient sur le cycle qui en découle.

use crate::domain::depense::Mois;
use chrono::{Datelike, NaiveDate};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("le jour de départ du mois doit être compris entre 1 et 31")]
pub struct JourDebutInvalide;

/// Jour du mois où commence un cycle budgétaire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JourDebutMois(u32);

impl JourDebutMois {
    pub const PREMIER: JourDebutMois = JourDebutMois(1);

    pub fn nouveau(jour: u32) -> Result<Self, JourDebutInvalide> {
        if (1..=31).contains(&jour) {
            Ok(Self(jour))
        } else {
            Err(JourDebutInvalide)
        }
    }

    pub fn valeur(&self) -> u32 {
        self.0
    }
}

impl Default for JourDebutMois {
    fn default() -> Self {
        Self::PREMIER
    }
}

/// Période couverte par un mois budgétaire, bornes `[début, fin[`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CycleMensuel {
    mois: Mois,
    jour_debut: JourDebutMois,
}

/// Dernier jour existant d'un mois donné (28, 29, 30 ou 31).
fn dernier_jour(annee: i32, mois: u32) -> u32 {
    let (annee_suivante, mois_suivant) = if mois == 12 {
        (annee + 1, 1)
    } else {
        (annee, mois + 1)
    };
    NaiveDate::from_ymd_opt(annee_suivante, mois_suivant, 1)
        .expect("premier du mois suivant valide")
        .pred_opt()
        .expect("un jour avant le premier du mois existe")
        .day()
}

/// Le jour demandé, ramené au dernier jour du mois s'il n'y existe pas : un
/// départ au 31 doit tenir en février.
fn jour_dans_le_mois(annee: i32, mois: u32, jour: u32) -> NaiveDate {
    let jour = jour.min(dernier_jour(annee, mois));
    NaiveDate::from_ymd_opt(annee, mois, jour).expect("jour ramené dans les bornes du mois")
}

impl CycleMensuel {
    pub fn nouveau(mois: Mois, jour_debut: JourDebutMois) -> Self {
        Self { mois, jour_debut }
    }

    pub fn mois(&self) -> Mois {
        self.mois
    }

    /// Premier jour compris dans le cycle.
    ///
    /// Un cycle porte le nom du mois de son **dernier** jour. Départ au 1er,
    /// « août » va donc du 1er au 31 août ; départ au 28, il va du 28 juillet
    /// au 27 août — celui que le salaire de fin juillet finance.
    pub fn debut(&self) -> NaiveDate {
        let fin = self.fin_exclue();
        let (annee, mois) = mois_precedent(fin.year(), fin.month());
        jour_dans_le_mois(annee, mois, self.jour_debut.valeur())
    }

    /// Premier jour du cycle suivant : borne haute exclue.
    pub fn fin_exclue(&self) -> NaiveDate {
        let debut_mois = self.mois.premier_jour();
        if self.jour_debut == JourDebutMois::PREMIER {
            self.mois.premier_jour_mois_suivant()
        } else {
            jour_dans_le_mois(
                debut_mois.year(),
                debut_mois.month(),
                self.jour_debut.valeur(),
            )
        }
    }

    /// Même jour de départ, sur le mois précédent.
    pub fn precedent(&self) -> Self {
        Self {
            mois: self.mois.precedent(),
            jour_debut: self.jour_debut,
        }
    }
}

fn mois_precedent(annee: i32, mois: u32) -> (i32, u32) {
    if mois == 1 {
        (annee - 1, 12)
    } else {
        (annee, mois - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cycle(etiquette: &str, jour: u32) -> CycleMensuel {
        CycleMensuel::nouveau(
            Mois::parse(etiquette).expect("étiquette valide"),
            JourDebutMois::nouveau(jour).expect("jour valide"),
        )
    }

    fn date(annee: i32, mois: u32, jour: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(annee, mois, jour).expect("date valide")
    }

    #[test]
    fn depart_au_premier_couvre_le_mois_calendaire() {
        // Comportement historique : il ne doit pas bouger pour qui n'a rien réglé.
        let aout = cycle("2026-08", 1);
        assert_eq!(aout.debut(), date(2026, 8, 1));
        assert_eq!(aout.fin_exclue(), date(2026, 9, 1));
    }

    #[test]
    fn depart_au_28_decale_le_cycle_sur_le_mois_precedent() {
        let aout = cycle("2026-08", 28);
        assert_eq!(aout.debut(), date(2026, 7, 28));
        assert_eq!(aout.fin_exclue(), date(2026, 8, 28));
    }

    #[test]
    fn un_cycle_porte_le_nom_du_mois_de_son_dernier_jour() {
        for jour in [1, 5, 15, 28] {
            let aout = cycle("2026-08", jour);
            let dernier = aout.fin_exclue().pred_opt().expect("veille de la fin");
            assert_eq!(
                dernier.month(),
                8,
                "départ au {jour} : le dernier jour du cycle doit tomber en août"
            );
        }
    }

    #[test]
    fn les_cycles_successifs_se_touchent_sans_trou_ni_recouvrement() {
        let juillet = cycle("2026-07", 28);
        let aout = cycle("2026-08", 28);
        assert_eq!(juillet.fin_exclue(), aout.debut());
    }

    #[test]
    fn un_depart_au_31_est_ramene_au_dernier_jour_des_mois_courts() {
        // Mars, départ au 31 : le cycle commence en février, qui n'a pas de 31.
        let mars = cycle("2026-03", 31);
        assert_eq!(mars.debut(), date(2026, 2, 28));
        assert_eq!(mars.fin_exclue(), date(2026, 3, 31));

        let mars_bissextile = cycle("2024-03", 31);
        assert_eq!(mars_bissextile.debut(), date(2024, 2, 29));
    }

    #[test]
    fn le_cycle_precedent_garde_le_jour_de_depart() {
        let precedent = cycle("2026-01", 15).precedent();
        assert_eq!(precedent.debut(), date(2025, 11, 15));
        assert_eq!(precedent.fin_exclue(), date(2025, 12, 15));
    }

    #[test]
    fn le_jour_de_depart_refuse_les_valeurs_hors_bornes() {
        assert_eq!(JourDebutMois::nouveau(0), Err(JourDebutInvalide));
        assert_eq!(JourDebutMois::nouveau(32), Err(JourDebutInvalide));
        assert!(JourDebutMois::nouveau(1).is_ok());
        assert!(JourDebutMois::nouveau(31).is_ok());
    }
}
