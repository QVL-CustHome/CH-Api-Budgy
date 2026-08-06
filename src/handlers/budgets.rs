use crate::api::error::ApiError;
use crate::api::extractors::ApiQuery;
use crate::api::response::ListResponse;
use crate::domain::budget::{MoisBudget, MontantPrevu, NouveauBudget};
use crate::domain::category::CategoryId;
use crate::domain::compte::ProprietaireId;
use crate::domain::ports::ecriture::BudgetsWriteRepository;
use crate::domain::ports::lecture::{BudgetsReadRepository, DepensesReadRepository};
use crate::domain::reste_a_depenser::{
    calculer_reste_a_depenser, calculer_reste_a_depenser_predit, mediane,
};
use crate::extract::BudgyUser;
use crate::handlers::commun::{categories_par_id, parse_month};
use crate::handlers::dto::{BudgetDto, BudgetQuery, RemainingBudgetDto, UpsertBudgetRequest};
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;

/// Nombre de mois passés servant de base à la prédiction (médiane glissante).
const MOIS_HISTORIQUE: usize = 3;

pub async fn upsert_budget(
    user: BudgyUser,
    State(state): State<AppState>,
    Json(payload): Json<UpsertBudgetRequest>,
) -> Result<(StatusCode, Json<BudgetDto>), ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let montant_prevu = MontantPrevu::parse(payload.montant_cents)
        .map_err(|e| ApiError::validation(e.to_string()))?;
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

#[derive(Debug, Clone, Deserialize)]
pub struct RemainingBudgetQuery {
    pub month: Option<String>,
}

pub async fn remaining_budgets(
    user: BudgyUser,
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<RemainingBudgetQuery>,
) -> Result<Json<RemainingBudgetDto>, ApiError> {
    let mois = parse_month(query.month.as_deref())?;
    let proprietaire = ProprietaireId(user.owner_id().to_string());

    let budgets = state
        .budgets
        .lister_par_mois(&proprietaire, mois.premier_jour())
        .await?;
    let depenses = state
        .depenses
        .repartition_mensuelle_par_categorie(&proprietaire, mois)
        .await?;
    // Historique glissant : on prédit sur la médiane de plusieurs mois plutôt
    // que sur le seul mois précédent, pour lisser les dépenses exceptionnelles.
    let mut historique = Vec::with_capacity(MOIS_HISTORIQUE);
    let mut mois_precedent = mois;
    for _ in 0..MOIS_HISTORIQUE {
        mois_precedent = mois_precedent.precedent();
        historique.push(
            state
                .depenses
                .repartition_mensuelle_par_categorie(&proprietaire, mois_precedent)
                .await?,
        );
    }
    let categories = categories_par_id(&state, &proprietaire).await?;

    // Budgets définis -> calcul classique. Sinon, on PRÉDIT le budget de chaque
    // catégorie à partir de la médiane de ses dépenses passées.
    let reste = if budgets.is_empty() {
        calculer_reste_a_depenser_predit(&historique, &depenses, &categories)
    } else {
        calculer_reste_a_depenser(budgets, &depenses, &categories)
    };

    // Total global : médiane des dépenses mensuelles passées (prévu) vs dépenses
    // du mois courant (toutes, catégorisées ou non).
    let mut totaux_passes: Vec<i64> = historique.iter().map(|mois| mois.total_cents).collect();
    let total_prevu = mediane(&mut totaux_passes);
    let total_depense = depenses.total_cents;
    Ok(Json(RemainingBudgetDto::depuis(
        mois,
        reste,
        total_prevu,
        total_depense,
    )))
}
