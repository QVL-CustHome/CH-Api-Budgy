use crate::api::error::ApiError;
use crate::api::extractors::{ApiPath, ApiQuery};
use crate::api::query::ListQuery;
use crate::api::response::ListResponse;
use crate::domain::bank_account::BankAccountId;
use crate::domain::compte::ProprietaireId;
use crate::domain::ports::lecture::{
    ComptesBancairesReadRepository, Tranche, TransactionsBancairesReadRepository,
};
use crate::extract::BudgyUser;
use crate::handlers::dto::{BankAccountSummaryDto, BankTransactionDto};
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use uuid::Uuid;

pub async fn list_accounts(
    user: BudgyUser,
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<ListQuery>,
) -> Result<Json<ListResponse<BankAccountSummaryDto>>, ApiError> {
    let pagination = query.pagination()?;
    let proprietaire = ProprietaireId(user.owner_id().to_string());

    let resultat = state
        .bank_accounts
        .lister_avec_solde(
            &proprietaire,
            Tranche {
                limit: pagination.limit,
                offset: pagination.offset,
            },
        )
        .await?;

    let data = resultat
        .elements
        .into_iter()
        .map(BankAccountSummaryDto::from)
        .collect();
    Ok(Json(ListResponse::new(data, resultat.total)))
}

pub async fn get_account(
    user: BudgyUser,
    State(state): State<AppState>,
    ApiPath(account_id): ApiPath<Uuid>,
) -> Result<Json<BankAccountSummaryDto>, ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());

    let compte = state
        .bank_accounts
        .fetch_avec_solde(&proprietaire, &BankAccountId(account_id))
        .await?
        .ok_or_else(|| ApiError::not_found("compte introuvable"))?;

    Ok(Json(BankAccountSummaryDto::from(compte)))
}

pub async fn list_account_transactions(
    user: BudgyUser,
    State(state): State<AppState>,
    ApiPath(account_id): ApiPath<Uuid>,
    ApiQuery(query): ApiQuery<ListQuery>,
) -> Result<Json<ListResponse<BankTransactionDto>>, ApiError> {
    let pagination = query.pagination()?;
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let compte = BankAccountId(account_id);

    if !state
        .bank_accounts
        .appartient_au_proprietaire(&proprietaire, &compte)
        .await?
    {
        return Err(ApiError::not_found("compte introuvable"));
    }

    let resultat = state
        .bank_transactions
        .lister_par_compte(
            &proprietaire,
            &compte,
            Tranche {
                limit: pagination.limit,
                offset: pagination.offset,
            },
        )
        .await?;

    let data = resultat
        .elements
        .into_iter()
        .map(BankTransactionDto::from)
        .collect();
    Ok(Json(ListResponse::new(data, resultat.total)))
}
