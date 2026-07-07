use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use sha2::Sha256;

pub const NONCE_BYTES: usize = 24;
pub const TAG_BYTES: usize = 16;

const DEDUP_SUBKEY_CONTEXT: &[u8] = b"budgy:v1:dedup-key";

type HmacSha256 = Hmac<Sha256>;

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
    dedup_subkey: [u8; 32],
}

impl CryptoService {
    pub fn from_key(key: &[u8]) -> Result<Self, CryptoError> {
        let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| CryptoError::InvalidKey)?;
        let dedup_subkey = deriver_sous_cle(key, DEDUP_SUBKEY_CONTEXT)?;
        Ok(Self {
            cipher,
            dedup_subkey,
        })
    }

    pub fn dedup_key(&self, contexte: &[u8], identifiant_externe: &str) -> String {
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&self.dedup_subkey)
            .expect("HMAC-SHA256 accepte une clé de 32 octets");
        mac.update(contexte);
        mac.update(b":");
        mac.update(identifiant_externe.as_bytes());
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
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

fn deriver_sous_cle(cle_maitre: &[u8], contexte: &[u8]) -> Result<[u8; 32], CryptoError> {
    let mut mac =
        <HmacSha256 as Mac>::new_from_slice(cle_maitre).map_err(|_| CryptoError::InvalidKey)?;
    mac.update(contexte);
    Ok(mac.finalize().into_bytes().into())
}
