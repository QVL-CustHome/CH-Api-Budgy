use crate::api::error::ApiError;
use crate::api::extractors::{ApiPath, ApiQuery};
use crate::api::query::ListQuery;
use crate::api::response::ListResponse;
use crate::domain::compte::CompteId;
use crate::domain::ports::lecture::{
    ComptesReadRepository, ListeComptesQuery, OwnerRef, Tranche,
};
use crate::extract::BudgyUser;
use crate::handlers::dto::{AccountBalanceDto, AccountDto};
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use uuid::Uuid;

pub async fn list_accounts(
    user: BudgyUser,
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<ListQuery>,
) -> Result<Json<ListResponse<AccountDto>>, ApiError> {
    let pagination = query.pagination()?;

    let resultat = state
        .comptes
        .lister(ListeComptesQuery {
            owner: OwnerRef(user.owner_id().to_string()),
            tranche: Tranche {
                limit: pagination.limit,
                offset: pagination.offset,
            },
        })
        .await?;

    let data = resultat.elements.into_iter().map(AccountDto::from).collect();
    Ok(Json(ListResponse::new(data, resultat.total)))
}

pub async fn get_account_balance(
    user: BudgyUser,
    State(state): State<AppState>,
    ApiPath(account_id): ApiPath<Uuid>,
) -> Result<Json<AccountBalanceDto>, ApiError> {
    let owner = OwnerRef(user.owner_id().to_string());

    let compte = state
        .comptes
        .solde(&owner, &CompteId(account_id))
        .await?
        .ok_or_else(|| ApiError::not_found("compte introuvable"))?;

    Ok(Json(AccountBalanceDto::from(compte)))
}
