use crate::domain::category::CategoryId;
use crate::domain::compte::ProprietaireId;
use crate::domain::libelle::extraire_tiers;
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
    /// Vrai si le motif se retrouve dans le libellé, en comparant les **tiers**
    /// extraits de part et d'autre (via [`extraire_tiers`]) et non les libellés
    /// bruts. Sans ça, un motif dérivé du tiers (« CARTE INTERMARCHE ») ne
    /// matcherait jamais son libellé brut (« CARTE 07/07/26 INTERMARCHE CB*7513 »),
    /// où une date ou une référence s'intercale — le format variant de surcroît
    /// d'une banque à l'autre.
    pub fn correspond(&self, label: &str) -> bool {
        extraire_tiers(label)
            .to_lowercase()
            .contains(&extraire_tiers(&self.label_pattern).to_lowercase())
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
    fn correspond_malgre_date_et_masque_carte() {
        // Le motif est le tiers nettoyé ; il doit matcher le libellé brut même
        // quand une date et un masque de carte s'intercalent.
        let regle = regle("CARTE INTERMARCHE", 0, instant(0));
        assert!(regle.correspond("CARTE 07/07/26 INTERMARCHE CB*7513"));
    }

    #[test]
    fn correspond_independamment_de_la_date_et_de_la_reference() {
        // Deux occurrences du même marchand, dates et masques différents.
        let regle = regle("CARTE INTERMARCHE", 0, instant(0));
        assert!(regle.correspond("CARTE 07/07/26 INTERMARCHE CB*7513"));
        assert!(regle.correspond("CARTE 21/07/26 INTERMARCHE CB*9999"));
    }

    #[test]
    fn correspond_au_milieu_du_tiers() {
        assert!(regle("carrefour", 0, instant(0)).correspond("ACHAT CARREFOUR MARKET"));
    }

    #[test]
    fn correspond_ignore_la_casse_des_deux_cotes() {
        assert!(regle("CarreFour", 0, instant(0)).correspond("Achat CARREFOUR Market"));
    }

    #[test]
    fn ne_correspond_pas_quand_le_marchand_est_absent() {
        assert!(!regle("amazon", 0, instant(0)).correspond("CARTE 07/07/26 INTERMARCHE CB*7513"));
    }

    #[test]
    fn ne_correspond_pas_sur_le_seul_prefixe_operation() {
        // « ACHAT » est un préfixe d'opération : retiré des deux côtés, il ne doit
        // pas suffire à faire matcher n'importe quel achat.
        assert!(!regle("achat", 0, instant(0)).correspond("ACHAT CARREFOUR MARKET"));
    }

    #[test]
    fn selectionner_retourne_la_regle_correspondante_la_plus_prioritaire() {
        let regles = vec![
            regle("amazon", 10, instant(2)),
            regle("carrefour", 5, instant(1)),
            regle("market", 1, instant(0)),
        ];
        let choisie = selectionner_regle("ACHAT CARREFOUR MARKET", &regles).unwrap();
        assert_eq!(choisie.label_pattern, "carrefour");
    }

    #[test]
    fn selectionner_choisit_la_priorite_max_meme_en_derniere_position_du_slice() {
        let regles = vec![
            regle("carrefour", 1, instant(0)),
            regle("market", 3, instant(0)),
            regle("carte", 10, instant(0)),
        ];
        // Tiers du libellé = « CARTE CARREFOUR MARKET » : les trois motifs matchent.
        let choisie =
            selectionner_regle("CARTE 21/07/26 CARREFOUR MARKET CB*7513", &regles).unwrap();
        assert_eq!(choisie.label_pattern, "carte");
    }

    #[test]
    fn selectionner_prend_la_plus_recente_a_priorite_egale_quel_que_soit_l_ordre() {
        let regles = vec![
            regle("market", 5, instant(1)),
            regle("carrefour", 5, instant(10)),
        ];
        let choisie = selectionner_regle("ACHAT CARREFOUR MARKET", &regles).unwrap();
        assert_eq!(choisie.label_pattern, "carrefour");
    }

    #[test]
    fn selectionner_retourne_none_sans_correspondance() {
        let regles = vec![regle("amazon", 0, instant(0))];
        assert!(selectionner_regle("ACHAT CARREFOUR MARKET", &regles).is_none());
    }
}
