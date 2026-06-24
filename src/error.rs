use crate::api::error::{ApiError, ApiErrorCode};
use axum::response::{IntoResponse, Response};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("requête invalide : {0}")]
    Validation(String),
    #[error("token invalide ou expiré")]
    InvalidToken,
    #[error("{0}")]
    Forbidden(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("{0}")]
    NotFound(&'static str),
    #[error("erreur interne")]
    Internal,
}

impl From<AppError> for ApiError {
    fn from(error: AppError) -> Self {
        let message = error.to_string();
        let code = match error {
            AppError::Validation(_) => ApiErrorCode::BadRequest,
            AppError::InvalidToken => ApiErrorCode::Unauthorized,
            AppError::Forbidden(_) => ApiErrorCode::Forbidden,
            AppError::Conflict(_) => ApiErrorCode::Conflict,
            AppError::NotFound(_) => ApiErrorCode::NotFound,
            AppError::Internal => ApiErrorCode::Internal,
        };
        ApiError::new(code, message)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        ApiError::from(self).into_response()
    }
}
