use crate::domain::compte::ProprietaireId;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub const CATEGORY_NAME_MAX_LEN: usize = 30;
pub const DEFAULT_CATEGORY_COLOR: &str = "#607D8B";
pub const DEFAULT_CATEGORY_ICON: &str = "tag";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CategoryKind {
    Revenu,
    Depense,
}

impl CategoryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            CategoryKind::Revenu => "revenu",
            CategoryKind::Depense => "depense",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "revenu" => Some(CategoryKind::Revenu),
            "depense" => Some(CategoryKind::Depense),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CategoryValidationError {
    #[error("le nom de la catégorie est obligatoire")]
    NomVide,
    #[error("le nom de la catégorie ne peut pas dépasser 30 caractères")]
    NomTropLong,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryName(String);

impl CategoryName {
    pub fn parse(value: &str) -> Result<Self, CategoryValidationError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(CategoryValidationError::NomVide);
        }
        if trimmed.chars().count() > CATEGORY_NAME_MAX_LEN {
            return Err(CategoryValidationError::NomTropLong);
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct Category {
    pub id: CategoryId,
    pub owner_id: Option<ProprietaireId>,
    pub name: String,
    pub kind: CategoryKind,
    pub color: String,
    pub icon: String,
    pub created_at: DateTime<Utc>,
}

impl Category {
    pub fn est_par_defaut(&self) -> bool {
        self.owner_id.is_none()
    }
}

#[derive(Debug, Clone)]
pub struct NouvelleCategorie {
    pub proprietaire: ProprietaireId,
    pub name: CategoryName,
    pub kind: CategoryKind,
    pub color: String,
    pub icon: String,
}

#[derive(Debug, Clone)]
pub struct MiseAJourCategorie {
    pub name: CategoryName,
    pub kind: CategoryKind,
    pub color: String,
    pub icon: String,
}

pub fn couleur_ou_defaut(color: Option<String>) -> String {
    valeur_non_vide_ou(color, DEFAULT_CATEGORY_COLOR)
}

pub fn icone_ou_defaut(icon: Option<String>) -> String {
    valeur_non_vide_ou(icon, DEFAULT_CATEGORY_ICON)
}

fn valeur_non_vide_ou(valeur: Option<String>, defaut: &str) -> String {
    valeur
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| defaut.to_string())
}
