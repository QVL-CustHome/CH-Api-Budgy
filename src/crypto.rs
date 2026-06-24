use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand_core::{OsRng, RngCore};

pub const NONCE_BYTES: usize = 24;
pub const TAG_BYTES: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("clé de chiffrement invalide")]
    InvalidKey,
    #[error("chiffrement impossible")]
    Encryption,
    #[error("déchiffrement impossible")]
    Decryption,
    #[error("blob chiffré tronqué")]
    MalformedBlob,
}

pub struct CryptoService {
    cipher: XChaCha20Poly1305,
}

impl CryptoService {
    pub fn from_key(key: &[u8]) -> Result<Self, CryptoError> {
        let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| CryptoError::InvalidKey)?;
        Ok(Self { cipher })
    }

    pub fn encrypt(&self, plaintext: &[u8], aad: &str) -> Result<Vec<u8>, CryptoError> {
        let mut nonce_bytes = [0u8; NONCE_BYTES];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = XNonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| CryptoError::Encryption)?;

        let mut blob = Vec::with_capacity(NONCE_BYTES + ciphertext.len());
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&ciphertext);
        Ok(blob)
    }

    pub fn decrypt(&self, blob: &[u8], aad: &str) -> Result<Vec<u8>, CryptoError> {
        if blob.len() < NONCE_BYTES + TAG_BYTES {
            return Err(CryptoError::MalformedBlob);
        }

        let (nonce_bytes, ciphertext) = blob.split_at(NONCE_BYTES);
        let nonce = XNonce::from_slice(nonce_bytes);

        self.cipher
            .decrypt(
                nonce,
                Payload {
                    msg: ciphertext,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| CryptoError::Decryption)
    }
}
