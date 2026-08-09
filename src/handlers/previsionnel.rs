use crate::api::error::ApiError;
use crate::api::extractors::ApiQuery;
use crate::domain::agregation::{MOIS_HISTORIQUE, medianes_par_categorie};
use crate::domain::compte::ProprietaireId;
use crate::domain::ports::lecture::{ComptesBancairesReadRepository, DepensesReadRepository};
use crate::domain::previsionnel::{PrevisionsParCategorie, calculer_previsionnel};
use crate::domain::solde_consolide::SoldeConsolide;
use crate::extract::BudgyUser;
use crate::handlers::commun::{categories_par_id, parse_month};
use crate::handlers::dto::ForecastDto;
use crate::handlers::preferences::cycle_du_mois;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ForecastQuery {
    pub month: Option<String>,
}

/// Projette le solde à la fin du cycle budgétaire.
///
/// On assemble quatre lectures : le solde d'aujourd'hui, ce qui a déjà été
/// dépensé et reçu depuis le début du cycle, et ce que les cycles passés
/// laissent attendre pour un cycle complet. Le domaine en déduit ce qui reste
/// réellement à venir.
pub async fn get_forecast(
    user: BudgyUser,
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<ForecastQuery>,
) -> Result<Json<ForecastDto>, ApiError> {
    let mois = parse_month(query.month.as_deref())?;
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let cycle = cycle_du_mois(&state, &proprietaire, mois).await?;

    let comptes = state.bank_accounts.lister_soldes(&proprietaire).await?;
    let solde_actuel_cents = SoldeConsolide::consolider(comptes).total_cents;

    let depenses_realisees = state
        .depenses
        .repartition_mensuelle_par_categorie(&proprietaire, cycle)
        .await?;
    let revenus_realises = state
        .depenses
        .repartition_mensuelle_revenus_par_categorie(&proprietaire, cycle)
        .await?;

    // Médiane sur les cycles précédents plutôt que sur le seul cycle passé :
    // un achat exceptionnel ne doit pas devenir une prévision mensuelle.
    let mut historique_depenses = Vec::with_capacity(MOIS_HISTORIQUE);
    let mut historique_revenus = Vec::with_capacity(MOIS_HISTORIQUE);
    let mut precedent = cycle;
    for _ in 0..MOIS_HISTORIQUE {
        precedent = precedent.precedent();
        historique_depenses.push(
            state
                .depenses
                .repartition_mensuelle_par_categorie(&proprietaire, precedent)
                .await?,
        );
        historique_revenus.push(
            state
                .depenses
                .repartition_mensuelle_revenus_par_categorie(&proprietaire, precedent)
                .await?,
        );
    }

    let categories = categories_par_id(&state, &proprietaire).await?;
    let revenus: std::collections::HashSet<_> =
        crate::domain::previsionnel::categories_de_revenu(&categories);

    // Un crédit rangé dans une catégorie de dépense est un remboursement, pas
    // une rentrée d'argent : le sens de la catégorie tranche.
    let depenses_prevues: PrevisionsParCategorie = medianes_par_categorie(&historique_depenses)
        .into_iter()
        .filter(|(category_id, _)| !revenus.contains(category_id))
        .collect();
    let revenus_prevus: PrevisionsParCategorie = medianes_par_categorie(&historique_revenus)
        .into_iter()
        .filter(|(category_id, _)| revenus.contains(category_id))
        .collect();

    let previsionnel = calculer_previsionnel(
        solde_actuel_cents,
        depenses_prevues,
        revenus_prevus,
        &depenses_realisees,
        &revenus_realises,
        &categories,
    );
    Ok(Json(ForecastDto::depuis(mois, previsionnel)))
}
