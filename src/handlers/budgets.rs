use crate::api::error::ApiError;
use crate::api::extractors::ApiQuery;
use crate::api::response::ListResponse;
use crate::domain::budget::{MoisBudget, MontantPrevu, NouveauBudget};
use crate::domain::category::CategoryId;
use crate::domain::compte::ProprietaireId;
use crate::domain::ports::ecriture::BudgetsWriteRepository;
use crate::domain::ports::lecture::BudgetsReadRepository;
use crate::extract::BudgyUser;
use crate::handlers::dto::{BudgetDto, BudgetQuery, UpsertBudgetRequest};
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

pub async fn upsert_budget(
    user: BudgyUser,
    State(state): State<AppState>,
    Json(payload): Json<UpsertBudgetRequest>,
) -> Result<(StatusCode, Json<BudgetDto>), ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let montant_prevu =
        MontantPrevu::parse(payload.montant_cents).map_err(|e| ApiError::validation(e.to_string()))?;
    let mois = MoisBudget::parse(&payload.mois).map_err(|e| ApiError::validation(e.to_string()))?;

    let budget = state
        .budgets
        .enregistrer(NouveauBudget {
            proprietaire,
            category_id: CategoryId(payload.category_id),
            montant_prevu,
            mois,
        })
        .await?
        .ok_or_else(|| ApiError::not_found("catégorie introuvable"))?;

    Ok((StatusCode::CREATED, Json(BudgetDto::from(budget))))
}

pub async fn list_budgets(
    user: BudgyUser,
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<BudgetQuery>,
) -> Result<Json<ListResponse<BudgetDto>>, ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let mois = MoisBudget::parse(&query.mois).map_err(|e| ApiError::validation(e.to_string()))?;

    let budgets = state
        .budgets
        .lister_par_mois(&proprietaire, mois.premier_jour())
        .await?;

    let total = budgets.len() as u64;
    let data = budgets.into_iter().map(BudgetDto::from).collect();
    Ok(Json(ListResponse::new(data, total)))
}
