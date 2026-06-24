use crate::api::error::ApiError;
use crate::api::extractors::ApiQuery;
use crate::api::query::ListQuery;
use crate::api::response::ListResponse;
use crate::domain::compte::CompteId;
use crate::domain::ports::lecture::{
    ListeTransactionsQuery, OwnerRef, Tranche, TransactionsReadRepository,
};
use crate::extract::BudgyUser;
use crate::handlers::dto::TransactionDto;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;

pub async fn list_transactions(
    user: BudgyUser,
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<ListQuery>,
) -> Result<Json<ListResponse<TransactionDto>>, ApiError> {
    let pagination = query.pagination()?;
    let date_range = query.date_range()?;

    let resultat = state
        .transactions
        .lister(ListeTransactionsQuery {
            owner: OwnerRef(user.owner_id().to_string()),
            compte: query.account_id.map(CompteId),
            depuis: date_range.from,
            jusqua: date_range.to,
            tranche: Tranche {
                limit: pagination.limit,
                offset: pagination.offset,
            },
        })
        .await?;

    let data = resultat
        .elements
        .into_iter()
        .map(TransactionDto::from)
        .collect();
    Ok(Json(ListResponse::new(data, resultat.total)))
}
