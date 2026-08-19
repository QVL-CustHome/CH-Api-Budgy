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
    FiltreTransactionsProprietaire, ReglesCategorisationReadRepository, Tranche,
    TransactionsBancairesReadRepository,
};
use crate::domain::transaction_bancaire::{
    ChampTriTransaction, OrdreTri, SensTransaction, TriTransactions,
};
use crate::extract::BudgyUser;
use crate::handlers::dto::BankTransactionDto;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct RecategorizeResult {
    pub categorisees: u64,
}

/// Réconcilie la catégorisation des transactions encore non catégorisées du
/// propriétaire, en trois temps : (0) appariement des virements internes, pour
/// qu'un simple mouvement entre ses comptes ne soit pris ni pour une dépense ni
/// pour un revenu ; (1) ré-application de toutes les règles de libellé
/// (rattrapage des transactions synchronisées avant qu'une règle existe, et des
/// libellés désormais reconnus grâce au matching sur tiers) ; (2) mise en
/// « Salaire » des crédits restants. Idempotent : ne touche que les `none`,
/// n'écrase jamais un choix manuel ni une catégorisation par règle existante.
pub async fn recategoriser_credits(
    user: BudgyUser,
    State(state): State<AppState>,
) -> Result<Json<RecategorizeResult>, ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());

    state
        .bank_transactions
        .recalculer_transferts_internes(&proprietaire)
        .await?;

    let regles = state
        .regles_categorisation
        .lister_pour_proprietaire(&proprietaire)
        .await?;
    let mut categorisees = 0u64;
    for regle in &regles {
        categorisees += state
            .bank_transactions
            .appliquer_regle_retroactif(regle)
            .await?;
    }

    categorisees += state
        .bank_transactions
        .recategoriser_credits(&proprietaire)
        .await?;

    Ok(Json(RecategorizeResult { categorisees }))
}

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

    // Marquer les virements internes : la ligne reste affichée, mais l'API
    // dit clairement qu'elle ne compte pas dans les totaux.
    let internes = state
        .bank_transactions
        .ids_transferts_internes(&proprietaire)
        .await?;
    let data = resultat
        .elements
        .into_iter()
        .map(|transaction| {
            let interne = internes.contains(&transaction.id.0);
            let mut dto = BankTransactionDto::from(transaction);
            dto.is_internal_transfer = interne;
            dto
        })
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
