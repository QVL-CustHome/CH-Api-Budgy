use crate::api::error::ApiError;
use crate::domain::category::CategoryId;
use crate::domain::compte::ProprietaireId;
use crate::domain::ports::ecriture::ReglesCategorisationWriteRepository;
use crate::domain::regle_categorisation::{LabelPattern, NouvelleRegleCategorisation};
use crate::extract::BudgyUser;
use crate::handlers::dto::{CategorizationRuleDto, CreateCategorizationRuleRequest};
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

pub async fn create_rule(
    user: BudgyUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateCategorizationRuleRequest>,
) -> Result<(StatusCode, Json<CategorizationRuleDto>), ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let label_pattern = LabelPattern::parse(&payload.label_pattern)
        .map_err(|e| ApiError::validation(e.to_string()))?;

    let regle = state
        .regles_categorisation
        .creer(NouvelleRegleCategorisation {
            proprietaire,
            label_pattern,
            category_id: CategoryId(payload.category_id),
            priority: payload.priority.unwrap_or(0),
        })
        .await?
        .ok_or_else(|| ApiError::not_found("catégorie introuvable"))?;

    if let Err(erreur) = state
        .bank_transactions
        .appliquer_regle_retroactif(&regle)
        .await
    {
        tracing::warn!(
            erreur = %erreur,
            regle_id = %regle.id.0,
            "application rétroactive de la règle ignorée"
        );
    }

    Ok((
        StatusCode::CREATED,
        Json(CategorizationRuleDto::from(regle)),
    ))
}
