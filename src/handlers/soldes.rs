use crate::api::error::ApiError;
use crate::domain::compte::ProprietaireId;
use crate::domain::ports::lecture::ComptesBancairesReadRepository;
use crate::domain::solde_consolide::SoldeConsolide;
use crate::extract::BudgyUser;
use crate::handlers::dto::ConsolidatedBalanceDto;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;

pub async fn get_consolidated_balance(
    user: BudgyUser,
    State(state): State<AppState>,
) -> Result<Json<ConsolidatedBalanceDto>, ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());

    let comptes = state.bank_accounts.lister_soldes(&proprietaire).await?;
    let consolide = SoldeConsolide::consolider(comptes);

    Ok(Json(ConsolidatedBalanceDto::from(consolide)))
}
