use crate::api::error::ApiError;
use crate::domain::compte::ProprietaireId;
use crate::domain::cycle::{CycleMensuel, JourDebutMois};
use crate::domain::depense::Mois;
use crate::extract::BudgyUser;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct PreferencesDto {
    pub jour_debut_mois: u32,
}

#[derive(Debug, Deserialize)]
pub struct PreferencesRequest {
    pub jour_debut_mois: u32,
}

pub async fn get_preferences(
    user: BudgyUser,
    State(state): State<AppState>,
) -> Result<Json<PreferencesDto>, ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let jour = state.preferences.jour_debut_mois(&proprietaire).await?;
    Ok(Json(PreferencesDto {
        jour_debut_mois: jour.valeur(),
    }))
}

pub async fn update_preferences(
    user: BudgyUser,
    State(state): State<AppState>,
    Json(payload): Json<PreferencesRequest>,
) -> Result<Json<PreferencesDto>, ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let jour = JourDebutMois::nouveau(payload.jour_debut_mois)
        .map_err(|e| ApiError::validation(e.to_string()))?;
    let jour = state
        .preferences
        .definir_jour_debut_mois(&proprietaire, jour)
        .await?;
    Ok(Json(PreferencesDto {
        jour_debut_mois: jour.valeur(),
    }))
}

/// Construit le cycle d'un mois d'après le réglage de l'utilisateur.
///
/// Point de passage unique : tout calcul mensuel doit s'y référer, sinon un
/// écran continuerait de raisonner en mois calendaire pendant que les autres
/// suivent le jour choisi.
pub async fn cycle_du_mois(
    state: &AppState,
    proprietaire: &ProprietaireId,
    mois: Mois,
) -> Result<CycleMensuel, ApiError> {
    let jour = state.preferences.jour_debut_mois(proprietaire).await?;
    Ok(CycleMensuel::nouveau(mois, jour))
}
