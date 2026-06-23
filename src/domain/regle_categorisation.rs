use crate::domain::categorie::CategorieId;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegleCategorisationId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChampCible {
    Libelle,
    ReferenceExterne,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperateurCorrespondance {
    Contient,
    CommencePar,
    Egal,
}

#[derive(Debug, Clone)]
pub struct RegleCategorisation {
    pub id: RegleCategorisationId,
    pub champ: ChampCible,
    pub operateur: OperateurCorrespondance,
    pub motif: String,
    pub categorie: CategorieId,
    pub priorite: i32,
    pub active: bool,
}
