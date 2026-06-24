use crate::api::error::ApiError;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{FromRequestParts, Path, Query};
use axum::http::request::Parts;
use serde::de::DeserializeOwned;

pub struct ApiPath<T>(pub T);

impl<T, S> FromRequestParts<S> for ApiPath<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Path::<T>::from_request_parts(parts, state).await {
            Ok(Path(value)) => Ok(ApiPath(value)),
            Err(rejection) => Err(path_error(rejection)),
        }
    }
}

pub struct ApiQuery<T>(pub T);

impl<T, S> FromRequestParts<S> for ApiQuery<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Query::<T>::from_request_parts(parts, state).await {
            Ok(Query(value)) => Ok(ApiQuery(value)),
            Err(rejection) => Err(query_error(rejection)),
        }
    }
}

fn path_error(rejection: PathRejection) -> ApiError {
    ApiError::validation(format!("paramètre de chemin invalide : {rejection}"))
}

fn query_error(rejection: QueryRejection) -> ApiError {
    ApiError::validation(format!("paramètres de requête invalides : {rejection}"))
}
