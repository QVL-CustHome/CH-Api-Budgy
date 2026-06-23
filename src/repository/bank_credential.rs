use crate::crypto::{CryptoError, CryptoService};
use crate::db::Db;
use uuid::Uuid;

const KEY_VERSION: i16 = 1;

#[derive(Debug, thiserror::Error)]
pub enum BankCredentialError {
    #[error("erreur base de données : {0}")]
    Database(#[from] sqlx::Error),
    #[error("erreur cryptographique : {0}")]
    Crypto(#[from] CryptoError),
    #[error("token déchiffré non valide en UTF-8")]
    InvalidUtf8,
}

fn access_token_aad(owner_id: &str) -> String {
    format!("budgy:v1:{owner_id}:bank_credential:access_token")
}

pub async fn insert(
    db: &Db,
    crypto: &CryptoService,
    owner_id: &str,
    access_token_plaintext: &str,
) -> Result<Uuid, BankCredentialError> {
    let aad = access_token_aad(owner_id);
    let blob = crypto.encrypt(access_token_plaintext.as_bytes(), &aad)?;

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO budgy.bank_credential (owner_id, access_token, key_version) \
         VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(owner_id)
    .bind(blob)
    .bind(KEY_VERSION)
    .fetch_one(db)
    .await?;

    Ok(id)
}

pub async fn fetch(
    db: &Db,
    crypto: &CryptoService,
    id: Uuid,
) -> Result<Option<String>, BankCredentialError> {
    let Some((owner_id, blob)) = sqlx::query_as::<_, (String, Vec<u8>)>(
        "SELECT owner_id, access_token FROM budgy.bank_credential WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(db)
    .await?
    else {
        return Ok(None);
    };

    let aad = access_token_aad(&owner_id);
    let plaintext = crypto.decrypt(&blob, &aad)?;
    let token = String::from_utf8(plaintext).map_err(|_| BankCredentialError::InvalidUtf8)?;

    Ok(Some(token))
}
