use crate::api::error::ApiError;
use crate::api::extractors::ApiQuery;
use crate::api::query::{
    SortDirection, TransactionKindFilter, TransactionSortField, TransactionsQuery,
};
use crate::api::response::ListResponse;
use crate::domain::bank_account::BankAccountId;
use crate::domain::category::CategoryId;
use crate::domain::compte::ProprietaireId;
use crate::domain::ports::lecture::{
    FiltreTransactionsProprietaire, Tranche, TransactionsBancairesReadRepository,
};
use crate::domain::transaction_bancaire::{
    ChampTriTransaction, OrdreTri, SensTransaction, TriTransactions,
};
use crate::extract::BudgyUser;
use crate::handlers::dto::BankTransactionDto;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;

pub async fn list_transactions(
    user: BudgyUser,
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<TransactionsQuery>,
) -> Result<Json<ListResponse<BankTransactionDto>>, ApiError> {
    let pagination = query.pagination()?;
    let periode = query.date_range()?;
    let proprietaire = ProprietaireId(user.owner_id().to_string());

    let filtre = FiltreTransactionsProprietaire {
        compte: query.account_id.map(BankAccountId),
        categorie: query.category_id.map(CategoryId),
        debut: periode.from,
        fin: periode.to,
        sens: sens_filtre(query.r#type),
    };

    let resultat = state
        .bank_transactions
        .lister_pour_proprietaire(
            &proprietaire,
            filtre,
            tri(query.sort, query.order),
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

fn sens_filtre(kind: Option<TransactionKindFilter>) -> Option<SensTransaction> {
    kind.map(|kind| match kind {
        TransactionKindFilter::Credit => SensTransaction::Entree,
        TransactionKindFilter::Debit => SensTransaction::Sortie,
    })
}

fn tri(sort: Option<TransactionSortField>, order: Option<SortDirection>) -> TriTransactions {
    let champ = match sort.unwrap_or(TransactionSortField::Date) {
        TransactionSortField::Date => ChampTriTransaction::Date,
        TransactionSortField::Amount => ChampTriTransaction::Montant,
    };
    let ordre = match order.unwrap_or(SortDirection::Desc) {
        SortDirection::Asc => OrdreTri::Ascendant,
        SortDirection::Desc => OrdreTri::Descendant,
    };
    TriTransactions { champ, ordre }
}
