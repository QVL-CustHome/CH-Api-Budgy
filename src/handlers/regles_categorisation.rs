use crate::api::error::ApiError;
use crate::domain::bank_account::BankAccountId;
use crate::domain::category::CategoryId;
use crate::domain::compte::ProprietaireId;
use crate::domain::libelle::extraire_tiers;
use crate::domain::ports::ecriture::ReglesCategorisationWriteRepository;
use crate::domain::regle_categorisation::{LabelPattern, NouvelleRegleCategorisation};
use crate::domain::transaction_bancaire::TransactionBancaireId;
use crate::extract::BudgyUser;
use crate::handlers::dto::{
    CategorizationRuleDto, CategorizeTransactionRequest, CreateCategorizationRuleRequest,
};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

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

/// Crée une règle de catégorisation à partir d'une transaction existante :
/// l'app dérive elle-même le motif stable (le « tiers ») depuis le libellé, sans
/// aucune saisie de l'utilisateur, puis l'applique rétroactivement.
pub async fn create_rule_from_transaction(
    user: BudgyUser,
    State(state): State<AppState>,
    Path((account_id, transaction_id)): Path<(Uuid, Uuid)>,
    Json(payload): Json<CategorizeTransactionRequest>,
) -> Result<(StatusCode, Json<CategorizationRuleDto>), ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let compte = BankAccountId(account_id);
    let transaction = TransactionBancaireId(transaction_id);

    let label = state
        .bank_transactions
        .libelle_transaction(&proprietaire, &compte, &transaction)
        .await?
        .ok_or_else(|| ApiError::not_found("transaction introuvable"))?;

    let motif = extraire_tiers(&label);
    let label_pattern =
        LabelPattern::parse(&motif).map_err(|e| ApiError::validation(e.to_string()))?;

    let regle = state
        .regles_categorisation
        .creer(NouvelleRegleCategorisation {
            proprietaire,
            label_pattern,
            category_id: CategoryId(payload.category_id),
            priority: 0,
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
            "application rétroactive de la règle (depuis transaction) ignorée"
        );
    }

    Ok((
        StatusCode::CREATED,
        Json(CategorizationRuleDto::from(regle)),
    ))
}
