use crate::api::error::ApiError;
use crate::api::extractors::ApiQuery;
use crate::api::response::ListResponse;
use crate::domain::agregation::MOIS_HISTORIQUE;
use crate::domain::budget::{MoisBudget, MontantPrevu, NouveauBudget};
use crate::domain::category::CategoryId;
use crate::domain::compte::ProprietaireId;
use crate::domain::ports::ecriture::BudgetsWriteRepository;
use crate::domain::ports::lecture::{BudgetsReadRepository, DepensesReadRepository};
use crate::domain::reste_a_depenser::{
    calculer_reste_a_depenser, calculer_reste_a_depenser_predit,
};
use crate::extract::BudgyUser;
use crate::handlers::commun::{categories_par_id, parse_month};
use crate::handlers::dto::{BudgetDto, BudgetQuery, RemainingBudgetDto, UpsertBudgetRequest};
use crate::handlers::preferences::cycle_du_mois;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;

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

    let cycle = cycle_du_mois(&state, &proprietaire, mois).await?;

    let budgets = state
        .budgets
        .lister_par_mois(&proprietaire, mois.premier_jour())
        .await?;
    let depenses = state
        .depenses
        .repartition_mensuelle_par_categorie(&proprietaire, cycle)
        .await?;
    // Historique glissant : on prédit sur la médiane de plusieurs mois plutôt
    // que sur le seul mois précédent, pour lisser les dépenses exceptionnelles.
    let mut historique = Vec::with_capacity(MOIS_HISTORIQUE);
    let mut cycle_precedent = cycle;
    for _ in 0..MOIS_HISTORIQUE {
        cycle_precedent = cycle_precedent.precedent();
        historique.push(
            state
                .depenses
                .repartition_mensuelle_par_categorie(&proprietaire, cycle_precedent)
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

    // Total global : exactement la somme des lignes affichées. Y ajouter les
    // dépenses non catégorisées gonflerait un total que le détail ne permet pas
    // de recouper — une enveloppe ne vaut que pour ce qu'on sait rattacher.
    let total_prevu: i64 = reste
        .lignes
        .iter()
        .map(|ligne| ligne.montant_prevu_cents)
        .sum();
    let total_depense: i64 = reste.lignes.iter().map(|ligne| ligne.depense_cents).sum();
    Ok(Json(RemainingBudgetDto::depuis(
        mois,
        reste,
        total_prevu,
        total_depense,
    )))
}
