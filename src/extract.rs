use crate::error::AppError;
use crate::services::jwt::Claims;
use crate::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::header;
use axum::http::request::Parts;

const MAX_TOKEN_BYTES: usize = 8 * 1024;
const REQUIRED_ROLE: &str = "budgy";

pub struct BudgyUser(pub Claims);

impl BudgyUser {
    pub fn owner_id(&self) -> &str {
        &self.0.sub
    }

    pub fn roles(&self) -> &[String] {
        &self.0.roles
    }
}

impl FromRequestParts<AppState> for BudgyUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(parts).ok_or(AppError::InvalidToken)?;
        if token.len() > MAX_TOKEN_BYTES {
            return Err(AppError::InvalidToken);
        }
        let claims = state
            .jwt
            .validate(&token)
            .map_err(|_| AppError::InvalidToken)?;
        if !claims.has_role(REQUIRED_ROLE) {
            return Err(AppError::Forbidden("rôle budgy requis"));
        }
        Ok(BudgyUser(claims))
    }
}

fn bearer_token(parts: &Parts) -> Option<String> {
    let value = parts.headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?.trim();
    (!token.is_empty()).then(|| token.to_string())
}
