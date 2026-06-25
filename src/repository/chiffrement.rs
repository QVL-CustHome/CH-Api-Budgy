use crate::crypto::{CryptoError, CryptoService};
use crate::domain::ports::ecriture::EcritureError;

pub const KEY_VERSION: i16 = 1;

pub fn vers_ecriture_error(erreur: ChiffrementError) -> EcritureError {
    match erreur {
        ChiffrementError::Database(e) => EcritureError::Acces(e.to_string()),
        ChiffrementError::Crypto(e) => EcritureError::Protection(e.to_string()),
        ChiffrementError::InvalidUtf8 => {
            EcritureError::Protection("champ déchiffré non valide en UTF-8".to_string())
        }
        ChiffrementError::InvalidAmount => {
            EcritureError::Protection("montant déchiffré illisible".to_string())
        }
        ChiffrementError::UnknownEnum(v) => {
            EcritureError::Acces(format!("valeur de domaine inconnue : {v}"))
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ChiffrementError {
    #[error("erreur base de données : {0}")]
    Database(#[from] sqlx::Error),
    #[error("erreur cryptographique : {0}")]
    Crypto(#[from] CryptoError),
    #[error("champ déchiffré non valide en UTF-8")]
    InvalidUtf8,
    #[error("montant déchiffré illisible")]
    InvalidAmount,
    #[error("valeur de domaine inconnue : {0}")]
    UnknownEnum(String),
}

pub fn aad(owner_id: &str, table: &str, field: &str) -> String {
    format!("budgy:v1:{owner_id}:{table}:{field}")
}

pub fn chiffrer_texte(
    crypto: &CryptoService,
    owner_id: &str,
    table: &str,
    field: &str,
    plaintext: &str,
) -> Result<Vec<u8>, ChiffrementError> {
    Ok(crypto.encrypt(plaintext.as_bytes(), &aad(owner_id, table, field))?)
}

pub fn dechiffrer_texte(
    crypto: &CryptoService,
    owner_id: &str,
    table: &str,
    field: &str,
    blob: &[u8],
) -> Result<String, ChiffrementError> {
    let bytes = crypto.decrypt(blob, &aad(owner_id, table, field))?;
    String::from_utf8(bytes).map_err(|_| ChiffrementError::InvalidUtf8)
}

pub fn chiffrer_montant(
    crypto: &CryptoService,
    owner_id: &str,
    table: &str,
    field: &str,
    amount_cents: i64,
) -> Result<Vec<u8>, ChiffrementError> {
    let plaintext = amount_cents.to_string();
    chiffrer_texte(crypto, owner_id, table, field, &plaintext)
}

pub fn dechiffrer_montant(
    crypto: &CryptoService,
    owner_id: &str,
    table: &str,
    field: &str,
    blob: &[u8],
) -> Result<i64, ChiffrementError> {
    let texte = dechiffrer_texte(crypto, owner_id, table, field, blob)?;
    texte
        .parse::<i64>()
        .map_err(|_| ChiffrementError::InvalidAmount)
}
