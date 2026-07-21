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
    #[error("le motif de libellé ne peut pas dépasser 140 caractères")]
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

#[derive(Debug, Clone)]
pub struct NouvelleRegleCategorisation {
    pub proprietaire: ProprietaireId,
    pub label_pattern: LabelPattern,
    pub category_id: CategoryId,
    pub priority: i32,
}
