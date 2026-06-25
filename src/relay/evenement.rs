use crate::domain::compte::ProprietaireId;
use serde::Deserialize;

pub const TYPE_USER_DELETED: &str = "auth.user.deleted";

#[derive(Debug, thiserror::Error)]
pub enum EvenementError {
    #[error("payload illisible : {0}")]
    PayloadInvalide(String),
    #[error("type d'événement inattendu : {0}")]
    TypeInattendu(String),
    #[error("identifiant de propriétaire absent")]
    SubManquant,
}

#[derive(Debug, Deserialize)]
struct UserDeletedPayload {
    event_type: String,
    sub: String,
}

pub fn parser_user_deleted(payload: &[u8]) -> Result<ProprietaireId, EvenementError> {
    let brut: UserDeletedPayload =
        serde_json::from_slice(payload).map_err(|e| EvenementError::PayloadInvalide(e.to_string()))?;

    if brut.event_type != TYPE_USER_DELETED {
        return Err(EvenementError::TypeInattendu(brut.event_type));
    }

    let sub = brut.sub.trim();
    if sub.is_empty() {
        return Err(EvenementError::SubManquant);
    }

    Ok(ProprietaireId(sub.to_string()))
}
