use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategorieId(pub Uuid);

#[derive(Debug, Clone)]
pub struct Categorie {
    pub id: CategorieId,
    pub libelle: String,
    pub parent: Option<CategorieId>,
    pub couleur: Option<String>,
    pub systeme: bool,
}
