use crate::api::error::ApiError;
use crate::api::extractors::ApiQuery;
use crate::domain::compte::ProprietaireId;
use crate::domain::depense::Mois;
use crate::domain::ports::lecture::DepensesReadRepository;
use crate::extract::BudgyUser;
use crate::handlers::dto::MonthlyExpensesDto;
use crate::handlers::preferences::cycle_du_mois;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ExpensesQuery {
    pub month: Option<String>,
}

pub async fn expenses_by_category(
    user: BudgyUser,
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<ExpensesQuery>,
) -> Result<Json<MonthlyExpensesDto>, ApiError> {
    let mois = parse_month(query.month.as_deref())?;
    let proprietaire = ProprietaireId(user.owner_id().to_string());

    let cycle = cycle_du_mois(&state, &proprietaire, mois).await?;

    let repartition = state
        .depenses
        .repartition_mensuelle_par_categorie(&proprietaire, cycle)
        .await?;

    Ok(Json(MonthlyExpensesDto::depuis(mois, repartition)))
}

fn parse_month(valeur: Option<&str>) -> Result<Mois, ApiError> {
    let valeur = valeur
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            ApiError::validation("le paramètre month est obligatoire (format YYYY-MM)")
        })?;
    Mois::parse(valeur).map_err(|e| ApiError::validation(e.to_string()))
}
