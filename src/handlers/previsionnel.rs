use crate::api::error::ApiError;
use crate::api::extractors::ApiQuery;
use crate::domain::agregation::{MOIS_HISTORIQUE, medianes_par_categorie};
use crate::domain::compte::ProprietaireId;
use crate::domain::ports::lecture::{
    BudgetsReadRepository, DepensesReadRepository, RecurrentsReadRepository,
};
use crate::domain::previsionnel::calculer_previsionnel;
use crate::extract::BudgyUser;
use crate::handlers::commun::{categories_par_id, parse_month};
use crate::handlers::dto::ForecastDto;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ForecastQuery {
    pub month: Option<String>,
}

pub async fn get_forecast(
    user: BudgyUser,
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<ForecastQuery>,
) -> Result<Json<ForecastDto>, ApiError> {
    let mois = parse_month(query.month.as_deref())?;
    let proprietaire = ProprietaireId(user.owner_id().to_string());

    let budgets = state
        .budgets
        .lister_par_mois(&proprietaire, mois.premier_jour())
        .await?;
    let recurrents = state
        .bank_transactions
        .lister_recurrents_pour_proprietaire(&proprietaire)
        .await?;
    // Revenus récurrents : médiane des crédits des derniers mois, catégorie par
    // catégorie. Contourne la détection à montant fixe, aveugle à un salaire
    // dont le montant et le libellé changent chaque mois.
    let mut historique_revenus = Vec::with_capacity(MOIS_HISTORIQUE);
    let mut mois_precedent = mois;
    for _ in 0..MOIS_HISTORIQUE {
        mois_precedent = mois_precedent.precedent();
        historique_revenus.push(
            state
                .depenses
                .repartition_mensuelle_revenus_par_categorie(&proprietaire, mois_precedent)
                .await?,
        );
    }
    let revenus_par_categorie = medianes_par_categorie(&historique_revenus);

    let categories = categories_par_id(&state, &proprietaire).await?;

    let previsionnel =
        calculer_previsionnel(recurrents, revenus_par_categorie, budgets, &categories);
    Ok(Json(ForecastDto::depuis(mois, previsionnel)))
}
