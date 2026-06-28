use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum SignatureError {
    #[error("clé privée RS256 illisible")]
    ClePrivee,
    #[error("signature du message impossible")]
    Encodage,
}

pub struct SignataireRs256 {
    cle: EncodingKey,
}

impl SignataireRs256 {
    pub fn nouveau(cle_privee_pem: &[u8]) -> Result<Self, SignatureError> {
        let cle =
            EncodingKey::from_rsa_pem(cle_privee_pem).map_err(|_| SignatureError::ClePrivee)?;
        Ok(Self { cle })
    }

    pub fn signer<C>(&self, claims: &C) -> Result<String, SignatureError>
    where
        C: Serialize,
    {
        let mut header = Header::new(Algorithm::RS256);
        header.typ = Some("JWT".to_string());
        jsonwebtoken::encode(&header, claims, &self.cle).map_err(|_| SignatureError::Encodage)
    }
}

impl std::fmt::Debug for SignataireRs256 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignataireRs256").finish_non_exhaustive()
    }
}
