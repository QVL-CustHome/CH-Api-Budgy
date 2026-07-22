use crate::api::error::ApiError;
use crate::api::extractors::ApiQuery;
use crate::domain::compte::ProprietaireId;
use crate::domain::ports::lecture::{BudgetsReadRepository, RecurrentsReadRepository};
use crate::domain::previsionnel::calculer_previsionnel;
use crate::extract::BudgyUser;
use crate::handlers::commun::{categories_par_id, parse_month};
use crate::handlers::dto::ForecastDto;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ForecastQuery {
    pub month: Option<String>,
}

pub async fn get_forecast(
    user: BudgyUser,
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<ForecastQuery>,
) -> Result<Json<ForecastDto>, ApiError> {
    let mois = parse_month(query.month.as_deref())?;
    let proprietaire = ProprietaireId(user.owner_id().to_string());

    let budgets = state
        .budgets
        .lister_par_mois(&proprietaire, mois.premier_jour())
        .await?;
    let recurrents = state
        .bank_transactions
        .lister_recurrents_pour_proprietaire(&proprietaire)
        .await?;
    let categories = categories_par_id(&state, &proprietaire).await?;

    let previsionnel = calculer_previsionnel(recurrents, budgets, &categories);
    Ok(Json(ForecastDto::depuis(mois, previsionnel)))
}
