use crate::api::error::ApiError;
use crate::api::extractors::ApiPath;
use crate::api::response::ListResponse;
use crate::domain::compte::ProprietaireId;
use crate::domain::enveloppe::{
    DEFAULT_ENVELOPPE_COLOR, DEFAULT_ENVELOPPE_ICON, EnveloppeId, EnveloppeNom, MiseAJourEnveloppe,
    MontantEnveloppe, NouvelleEnveloppe, SuiviEnveloppe,
};
use crate::extract::BudgyUser;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct EnveloppeDto {
    pub id: Uuid,
    pub nom: String,
    pub icon: String,
    pub color: String,
    pub montant_cents: i64,
    pub depense_cents: i64,
    pub restant_cents: i64,
    pub pourcentage_consomme: u8,
    pub depasse: bool,
    pub nombre_transactions: i64,
}

impl From<SuiviEnveloppe> for EnveloppeDto {
    fn from(suivi: SuiviEnveloppe) -> Self {
        Self {
            id: suivi.enveloppe.id.0,
            nom: suivi.enveloppe.nom.clone(),
            icon: suivi.enveloppe.icon.clone(),
            color: suivi.enveloppe.color.clone(),
            montant_cents: suivi.enveloppe.montant_cents,
            depense_cents: suivi.depense_cents,
            restant_cents: suivi.restant_cents(),
            pourcentage_consomme: suivi.pourcentage_consomme(),
            depasse: suivi.depasse(),
            nombre_transactions: suivi.nombre_transactions,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct EnveloppeRequest {
    pub nom: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub montant_cents: i64,
}

#[derive(Debug, Deserialize)]
pub struct AffectationRequest {
    /// `None` retire la transaction de son enveloppe.
    pub enveloppe_id: Option<Uuid>,
}

fn valeur_ou_defaut(valeur: Option<String>, defaut: &str) -> String {
    valeur
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| defaut.to_string())
}

fn parse_requete(
    payload: &EnveloppeRequest,
) -> Result<(EnveloppeNom, MontantEnveloppe, String, String), ApiError> {
    let nom = EnveloppeNom::parse(&payload.nom).map_err(|e| ApiError::validation(e.to_string()))?;
    let montant = MontantEnveloppe::parse(payload.montant_cents)
        .map_err(|e| ApiError::validation(e.to_string()))?;
    Ok((
        nom,
        montant,
        valeur_ou_defaut(payload.icon.clone(), DEFAULT_ENVELOPPE_ICON),
        valeur_ou_defaut(payload.color.clone(), DEFAULT_ENVELOPPE_COLOR),
    ))
}

pub async fn list_enveloppes(
    user: BudgyUser,
    State(state): State<AppState>,
) -> Result<Json<ListResponse<EnveloppeDto>>, ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let enveloppes = state.enveloppes.lister(&proprietaire).await?;
    let total = enveloppes.len() as u64;
    let data = enveloppes.into_iter().map(EnveloppeDto::from).collect();
    Ok(Json(ListResponse::new(data, total)))
}

pub async fn create_enveloppe(
    user: BudgyUser,
    State(state): State<AppState>,
    Json(payload): Json<EnveloppeRequest>,
) -> Result<(StatusCode, Json<EnveloppeDto>), ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let (nom, montant, icon, color) = parse_requete(&payload)?;

    let enveloppe = state
        .enveloppes
        .creer(NouvelleEnveloppe {
            proprietaire: proprietaire.clone(),
            nom,
            icon,
            color,
            montant,
        })
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(EnveloppeDto::from(SuiviEnveloppe {
            enveloppe,
            depense_cents: 0,
            nombre_transactions: 0,
        })),
    ))
}

pub async fn update_enveloppe(
    user: BudgyUser,
    State(state): State<AppState>,
    ApiPath(enveloppe_id): ApiPath<Uuid>,
    Json(payload): Json<EnveloppeRequest>,
) -> Result<Json<EnveloppeDto>, ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let (nom, montant, icon, color) = parse_requete(&payload)?;

    let enveloppe = state
        .enveloppes
        .modifier(
            &proprietaire,
            &EnveloppeId(enveloppe_id),
            MiseAJourEnveloppe {
                nom,
                icon,
                color,
                montant,
            },
        )
        .await?
        .ok_or_else(|| ApiError::not_found("budget introuvable"))?;

    // La consommation est relue : modifier le montant change le restant.
    let suivi = state
        .enveloppes
        .lister(&proprietaire)
        .await?
        .into_iter()
        .find(|s| s.enveloppe.id == enveloppe.id)
        .unwrap_or(SuiviEnveloppe {
            enveloppe,
            depense_cents: 0,
            nombre_transactions: 0,
        });

    Ok(Json(EnveloppeDto::from(suivi)))
}

pub async fn delete_enveloppe(
    user: BudgyUser,
    State(state): State<AppState>,
    ApiPath(enveloppe_id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let supprimee = state
        .enveloppes
        .supprimer(&proprietaire, &EnveloppeId(enveloppe_id))
        .await?;
    if !supprimee {
        return Err(ApiError::not_found("budget introuvable"));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn affecter_transaction(
    user: BudgyUser,
    State(state): State<AppState>,
    ApiPath(transaction_id): ApiPath<Uuid>,
    Json(payload): Json<AffectationRequest>,
) -> Result<StatusCode, ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let enveloppe = payload.enveloppe_id.map(EnveloppeId);

    let affectee = state
        .enveloppes
        .affecter_transaction(&proprietaire, transaction_id, enveloppe.as_ref())
        .await?;
    if !affectee {
        return Err(ApiError::not_found("transaction ou budget introuvable"));
    }
    Ok(StatusCode::NO_CONTENT)
}
