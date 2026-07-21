use crate::domain::category::CategoryId;
use crate::domain::compte::ProprietaireId;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub const LABEL_PATTERN_MAX_LEN: usize = 140;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegleCategorisationId(pub Uuid);

#[derive(Debug, thiserror::Error)]
pub enum RegleValidationError {
    #[error("le motif de libellé est obligatoire")]
    MotifVide,
    #[error("le motif de libellé ne peut pas dépasser {LABEL_PATTERN_MAX_LEN} caractères")]
    MotifTropLong,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelPattern(String);

impl LabelPattern {
    pub fn parse(value: &str) -> Result<Self, RegleValidationError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(RegleValidationError::MotifVide);
        }
        if trimmed.chars().count() > LABEL_PATTERN_MAX_LEN {
            return Err(RegleValidationError::MotifTropLong);
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct RegleCategorisation {
    pub id: RegleCategorisationId,
    pub owner_id: ProprietaireId,
    pub label_pattern: String,
    pub category_id: CategoryId,
    pub priority: i32,
    pub created_at: DateTime<Utc>,
}

impl RegleCategorisation {
    pub fn correspond(&self, label: &str) -> bool {
        label
            .to_lowercase()
            .contains(&self.label_pattern.to_lowercase())
    }
}

pub fn selectionner_regle<'a>(
    label: &str,
    regles: &'a [RegleCategorisation],
) -> Option<&'a RegleCategorisation> {
    regles
        .iter()
        .filter(|regle| regle.correspond(label))
        .max_by_key(|regle| (regle.priority, regle.created_at, regle.id.0))
}

#[derive(Debug, Clone)]
pub struct NouvelleRegleCategorisation {
    pub proprietaire: ProprietaireId,
    pub label_pattern: LabelPattern,
    pub category_id: CategoryId,
    pub priority: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn regle(label_pattern: &str, priority: i32, created_at: DateTime<Utc>) -> RegleCategorisation {
        RegleCategorisation {
            id: RegleCategorisationId(Uuid::new_v4()),
            owner_id: ProprietaireId("owner".to_string()),
            label_pattern: label_pattern.to_string(),
            category_id: CategoryId(Uuid::new_v4()),
            priority,
            created_at,
        }
    }

    fn instant(secondes: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secondes, 0).unwrap()
    }

    #[test]
    fn correspond_en_debut_de_libelle() {
        assert!(regle("achat", 0, instant(0)).correspond("achat carrefour market"));
    }

    #[test]
    fn correspond_au_milieu_du_libelle() {
        assert!(regle("carrefour", 0, instant(0)).correspond("achat carrefour market"));
    }

    #[test]
    fn correspond_en_fin_de_libelle() {
        assert!(regle("market", 0, instant(0)).correspond("achat carrefour market"));
    }

    #[test]
    fn correspond_ignore_la_casse_des_deux_cotes() {
        assert!(regle("CarreFour", 0, instant(0)).correspond("Achat CARREFOUR Market"));
    }

    #[test]
    fn ne_correspond_pas_quand_le_motif_est_absent() {
        assert!(!regle("amazon", 0, instant(0)).correspond("achat carrefour market"));
    }

    #[test]
    fn selectionner_retourne_la_premiere_regle_correspondante() {
        let regles = vec![
            regle("amazon", 10, instant(2)),
            regle("carrefour", 5, instant(1)),
            regle("market", 1, instant(0)),
        ];
        let choisie = selectionner_regle("achat carrefour market", &regles).unwrap();
        assert_eq!(choisie.label_pattern, "carrefour");
    }

    #[test]
    fn selectionner_respecte_l_ordre_pour_les_egalites() {
        let regles = vec![
            regle("carrefour", 5, instant(2)),
            regle("carrefour market", 5, instant(1)),
        ];
        let choisie = selectionner_regle("achat carrefour market", &regles).unwrap();
        assert_eq!(choisie.label_pattern, "carrefour");
    }

    #[test]
    fn selectionner_choisit_la_priorite_max_meme_en_derniere_position_du_slice() {
        let regles = vec![
            regle("carrefour", 1, instant(0)),
            regle("market", 3, instant(0)),
            regle("achat", 10, instant(0)),
        ];
        let choisie = selectionner_regle("achat carrefour market", &regles).unwrap();
        assert_eq!(choisie.label_pattern, "achat");
    }

    #[test]
    fn selectionner_prend_la_plus_recente_a_priorite_egale_quel_que_soit_l_ordre() {
        let regles = vec![
            regle("achat", 5, instant(1)),
            regle("carrefour", 5, instant(10)),
        ];
        let choisie = selectionner_regle("achat carrefour market", &regles).unwrap();
        assert_eq!(choisie.label_pattern, "carrefour");
    }

    #[test]
    fn selectionner_retourne_none_sans_correspondance() {
        let regles = vec![regle("amazon", 0, instant(0))];
        assert!(selectionner_regle("achat carrefour market", &regles).is_none());
    }
}
