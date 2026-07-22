use crate::api::error::ApiError;
use crate::domain::category::Category;
use crate::domain::compte::ProprietaireId;
use crate::domain::depense::Mois;
use crate::domain::ports::lecture::CategoriesReadRepository;
use crate::state::AppState;
use std::collections::HashMap;
use uuid::Uuid;

pub async fn categories_par_id(
    state: &AppState,
    proprietaire: &ProprietaireId,
) -> Result<HashMap<Uuid, Category>, ApiError> {
    let categories = state
        .categories
        .lister_pour_proprietaire(proprietaire)
        .await?
        .into_iter()
        .map(|item| (item.category.id.0, item.category))
        .collect();
    Ok(categories)
}

pub fn parse_month(valeur: Option<&str>) -> Result<Mois, ApiError> {
    let valeur = valeur
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            ApiError::validation("le paramètre month est obligatoire (format YYYY-MM)")
        })?;
    Mois::parse(valeur).map_err(|e| ApiError::validation(e.to_string()))
}
