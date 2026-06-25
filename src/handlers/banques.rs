use crate::api::error::ApiError;
use crate::api::response::ListResponse;
use crate::domain::bank_account::NouveauBankAccount;
use crate::domain::compte::ProprietaireId;
use crate::domain::consent::{ConsentId, ConsentStatus, MiseAJourConsent, NouveauConsentInitie};
use crate::domain::ports::bank_data_source::{
    BankDataSourceError, DemandeConsentement, ReponseAutorisation,
};
use crate::domain::ports::ecriture::{BankAccountsWriteRepository, ConsentsWriteRepository};
use crate::domain::ports::lecture::{BankAccountsReadRepository, ConsentsReadRepository};
use crate::extract::BudgyUser;
use crate::handlers::dto::{
    BankAccountDto, BankDto, ConsentCallbackRequest, ConsentCompletionDto, ConsentDto,
    CreateConsentRequest, CreateConsentResponse,
};
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use uuid::Uuid;

pub async fn list_banks(
    _user: BudgyUser,
    State(state): State<AppState>,
) -> Result<Json<ListResponse<BankDto>>, ApiError> {
    let etablissements = state.bank_source.lister_etablissements().await?;
    let total = etablissements.len() as u64;
    let data = etablissements.into_iter().map(BankDto::from).collect();
    Ok(Json(ListResponse::new(data, total)))
}

pub async fn create_consent(
    user: BudgyUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateConsentRequest>,
) -> Result<Json<CreateConsentResponse>, ApiError> {
    let bank_id = payload.bank_id.trim();
    if bank_id.is_empty() {
        return Err(ApiError::validation("bank_id requis"));
    }

    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let consent_id = ConsentId(Uuid::new_v4());

    let initie = state
        .bank_source
        .initier_consentement(DemandeConsentement {
            consent_id: consent_id.clone(),
            proprietaire: proprietaire.clone(),
            etablissement: bank_id.to_string(),
            url_retour: state.bank_callback_url.clone(),
        })
        .await?;

    state
        .consents
        .enregistrer_initie(NouveauConsentInitie {
            id: initie.consent.id.clone(),
            proprietaire,
            external_ref: initie.consent.external_ref.clone(),
            status: ConsentStatus::Pending,
            expires_at: initie.consent.expires_at,
        })
        .await?;

    Ok(Json(CreateConsentResponse {
        consent_id: initie.consent.id.0,
        authorization_url: initie.url_autorisation,
    }))
}

pub async fn complete_consent(
    user: BudgyUser,
    State(state): State<AppState>,
    Json(payload): Json<ConsentCallbackRequest>,
) -> Result<Json<ConsentCompletionDto>, ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let consent_id = parse_consent_id(&payload.state)?;

    let consent = state
        .consents
        .fetch_pour_proprietaire(&proprietaire, &consent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("consentement introuvable"))?;

    if consent.status != ConsentStatus::Pending {
        return completion_existante(&state, &proprietaire, &consent_id, consent.status).await;
    }

    let actif = match state
        .bank_source
        .completer_consentement(
            &proprietaire,
            ReponseAutorisation {
                reference_autorisation: payload.state.clone(),
                code_autorisation: payload.code.clone(),
            },
        )
        .await
    {
        Ok(consent) => consent,
        Err(erreur) => {
            return Err(echec_consentement(&state, &proprietaire, &consent_id, erreur).await);
        }
    };

    state
        .consents
        .mettre_a_jour(
            &proprietaire,
            &consent_id,
            MiseAJourConsent {
                status: ConsentStatus::Active,
                external_ref: actif.external_ref.clone(),
                expires_at: actif.expires_at,
            },
        )
        .await?;

    let comptes_bancaires = state.bank_source.lister_comptes(&actif).await?;
    for compte in &comptes_bancaires {
        state
            .bank_accounts
            .enregistrer(NouveauBankAccount {
                proprietaire: proprietaire.clone(),
                consent: consent_id.clone(),
                external_account_id: compte.external_account_id.clone(),
                iban: compte.iban_masked.clone(),
                currency: compte.currency.clone(),
                next_sync_at: compte.next_sync_at,
            })
            .await?;
    }

    let persistes = state
        .bank_accounts
        .lister_par_consent(&proprietaire, &consent_id)
        .await?;

    Ok(Json(ConsentCompletionDto {
        consent_id: consent_id.0,
        status: ConsentStatus::Active.into(),
        comptes: persistes.into_iter().map(BankAccountDto::from).collect(),
    }))
}

pub async fn list_consents(
    user: BudgyUser,
    State(state): State<AppState>,
) -> Result<Json<ListResponse<ConsentDto>>, ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let consents = state.consents.lister_par_proprietaire(&proprietaire).await?;
    let total = consents.len() as u64;
    let data = consents.into_iter().map(ConsentDto::from).collect();
    Ok(Json(ListResponse::new(data, total)))
}

async fn completion_existante(
    state: &AppState,
    proprietaire: &ProprietaireId,
    consent_id: &ConsentId,
    status: ConsentStatus,
) -> Result<Json<ConsentCompletionDto>, ApiError> {
    let persistes = state
        .bank_accounts
        .lister_par_consent(proprietaire, consent_id)
        .await?;

    Ok(Json(ConsentCompletionDto {
        consent_id: consent_id.0,
        status: status.into(),
        comptes: persistes.into_iter().map(BankAccountDto::from).collect(),
    }))
}

fn parse_consent_id(state: &str) -> Result<ConsentId, ApiError> {
    Uuid::parse_str(state.trim())
        .map(ConsentId)
        .map_err(|_| ApiError::validation("state invalide"))
}

async fn echec_consentement(
    state: &AppState,
    proprietaire: &ProprietaireId,
    consent_id: &ConsentId,
    erreur: BankDataSourceError,
) -> ApiError {
    if let Err(ecriture) = state
        .consents
        .marquer_statut(proprietaire, consent_id, ConsentStatus::Failed)
        .await
    {
        return ApiError::from(ecriture);
    }
    ApiError::from(erreur)
}
