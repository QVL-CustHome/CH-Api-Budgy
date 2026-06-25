use chrono::Utc;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::Serialize;
use std::sync::Mutex;

const ISSUER: &str = "enablebanking.com";
const AUDIENCE: &str = "api.enablebanking.com";
const TTL_SECONDES: i64 = 3600;
const MARGE_RENOUVELLEMENT_SECONDES: i64 = 60;

#[derive(Debug, thiserror::Error)]
pub enum JwtApplicatifError {
    #[error("clé privée RSA illisible")]
    ClePrivee,
    #[error("signature du jeton impossible")]
    Signature,
}

#[derive(Serialize)]
struct ClaimsApplicatives {
    iss: &'static str,
    aud: &'static str,
    iat: i64,
    exp: i64,
}

struct JetonCache {
    valeur: String,
    expire_a: i64,
}

pub struct SignataireJwt {
    app_id: String,
    cle: EncodingKey,
    cache: Mutex<Option<JetonCache>>,
}

impl SignataireJwt {
    pub fn nouveau(app_id: &str, cle_privee_pem: &[u8]) -> Result<Self, JwtApplicatifError> {
        let cle =
            EncodingKey::from_rsa_pem(cle_privee_pem).map_err(|_| JwtApplicatifError::ClePrivee)?;
        Ok(Self {
            app_id: app_id.to_string(),
            cle,
            cache: Mutex::new(None),
        })
    }

    pub fn jeton(&self) -> Result<String, JwtApplicatifError> {
        let maintenant = Utc::now().timestamp();
        if let Some(jeton) = self.jeton_en_cache(maintenant) {
            return Ok(jeton);
        }

        let expire_a = maintenant + TTL_SECONDES;
        let jeton = self.signer(maintenant, expire_a)?;

        if let Ok(mut cache) = self.cache.lock() {
            *cache = Some(JetonCache {
                valeur: jeton.clone(),
                expire_a,
            });
        }
        Ok(jeton)
    }

    fn jeton_en_cache(&self, maintenant: i64) -> Option<String> {
        let cache = self.cache.lock().ok()?;
        let jeton = cache.as_ref()?;
        if jeton.expire_a - MARGE_RENOUVELLEMENT_SECONDES > maintenant {
            Some(jeton.valeur.clone())
        } else {
            None
        }
    }

    fn signer(&self, iat: i64, exp: i64) -> Result<String, JwtApplicatifError> {
        let mut header = Header::new(Algorithm::RS256);
        header.typ = Some("JWT".to_string());
        header.kid = Some(self.app_id.clone());

        let claims = ClaimsApplicatives {
            iss: ISSUER,
            aud: AUDIENCE,
            iat,
            exp,
        };

        jsonwebtoken::encode(&header, &claims, &self.cle).map_err(|_| JwtApplicatifError::Signature)
    }
}

impl std::fmt::Debug for SignataireJwt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignataireJwt").finish_non_exhaustive()
    }
}
