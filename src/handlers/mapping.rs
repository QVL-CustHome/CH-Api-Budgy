use crate::api::error::ApiError;
use crate::domain::ports::bank_data_source::BankDataSourceError;
use crate::domain::ports::ecriture::EcritureError;
use crate::domain::ports::lecture::LectureError;

impl From<LectureError> for ApiError {
    fn from(error: LectureError) -> Self {
        match error {
            LectureError::Acces(_) => ApiError::internal("erreur d'accès aux données"),
        }
    }
}

impl From<EcritureError> for ApiError {
    fn from(error: EcritureError) -> Self {
        match error {
            EcritureError::Acces(_) => ApiError::internal("erreur d'écriture des données"),
            EcritureError::Protection(_) => ApiError::internal("protection des données impossible"),
        }
    }
}

impl From<BankDataSourceError> for ApiError {
    fn from(error: BankDataSourceError) -> Self {
        match error {
            BankDataSourceError::ConsentementInvalide => {
                ApiError::consent_refused("consentement bancaire refusé ou expiré")
            }
            BankDataSourceError::EtablissementIndisponible => {
                ApiError::bank_unavailable("établissement bancaire momentanément indisponible")
            }
            BankDataSourceError::RessourceIntrouvable => {
                ApiError::not_found("ressource bancaire introuvable")
            }
            BankDataSourceError::SourceNonConfiguree => {
                ApiError::internal("source bancaire non configurée")
            }
            BankDataSourceError::ReponseInvalide(_) => {
                ApiError::bank_unavailable("réponse de l'établissement bancaire illisible")
            }
            BankDataSourceError::Technique(_) => {
                ApiError::bank_unavailable("erreur de communication avec l'établissement bancaire")
            }
        }
    }
}
