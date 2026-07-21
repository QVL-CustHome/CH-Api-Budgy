use chrono::{DateTime, Utc};
use uuid::Uuid;

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

#[derive(Debug, Clone)]
pub struct Category {
    pub id: CategoryId,
    pub name: String,
    pub kind: CategoryKind,
    pub color: String,
    pub icon: String,
    pub created_at: DateTime<Utc>,
}
