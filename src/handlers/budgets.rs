use crate::api::error::ApiError;
use crate::api::extractors::ApiQuery;
use crate::api::response::ListResponse;
use crate::domain::budget::{MoisBudget, MontantPrevu, NouveauBudget};
use crate::domain::category::{Category, CategoryId};
use crate::domain::compte::ProprietaireId;
use crate::domain::depense::Mois;
use crate::domain::ports::ecriture::BudgetsWriteRepository;
use crate::domain::ports::lecture::{
    BudgetsReadRepository, CategoriesReadRepository, DepensesReadRepository,
};
use crate::domain::reste_a_depenser::calculer_reste_a_depenser;
use crate::extract::BudgyUser;
use crate::handlers::dto::{BudgetDto, BudgetQuery, RemainingBudgetDto, UpsertBudgetRequest};
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use std::collections::HashMap;
use uuid::Uuid;

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
    let categories = categories_par_id(&state, &proprietaire).await?;

    let reste = calculer_reste_a_depenser(budgets, &depenses, &categories);
    Ok(Json(RemainingBudgetDto::depuis(mois, reste)))
}

async fn categories_par_id(
    state: &AppState,
    proprietaire: &ProprietaireId,
) -> Result<HashMap<Uuid, Category>, ApiError> {
    let categories = state
        .categories
        .lister_pour_proprietaire(proprietaire)
        .await?
        .into_iter()
        .map(|item| (item.category.id.0, item.category))
        .collect();
    Ok(categories)
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
