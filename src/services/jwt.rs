use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{HashMap, HashSet};

pub const ALGORITHM: Algorithm = Algorithm::HS256;

#[derive(Debug, thiserror::Error)]
pub enum JwtValidationError {
    #[error("token invalide ou expiré")]
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Claims {
    pub sub: String,

    #[serde(default, deserialize_with = "deserialize_roles")]
    pub roles: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,

    #[serde(default, deserialize_with = "deserialize_audience")]
    pub aud: Vec<String>,

    pub iat: u64,
    pub exp: u64,
}

impl Claims {
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|owned| owned == role)
    }
}

fn deserialize_audience<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum AudienceFormat {
        One(String),
        Many(Vec<String>),
    }

    Ok(match Option::<AudienceFormat>::deserialize(deserializer)? {
        None => Vec::new(),
        Some(AudienceFormat::One(value)) => vec![value],
        Some(AudienceFormat::Many(values)) => values,
    })
}

fn deserialize_roles<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum RolesFormat {
        Flat(Vec<String>),
        PerPortalMany(HashMap<String, Vec<String>>),
        PerPortalOne(HashMap<String, String>),
    }

    let collected = match RolesFormat::deserialize(deserializer)? {
        RolesFormat::Flat(roles) => roles,
        RolesFormat::PerPortalMany(map) => map.into_values().flatten().collect(),
        RolesFormat::PerPortalOne(map) => map.into_values().collect(),
    };

    let mut seen = HashSet::new();
    Ok(collected
        .into_iter()
        .filter(|role| seen.insert(role.clone()))
        .collect())
}

pub struct JwtService {
    decoding: DecodingKey,
    validation: Validation,
}

impl JwtService {
    pub fn from_secret(secret: &str, issuer: &str, audience: &str) -> Self {
        let mut validation = Validation::new(ALGORITHM);
        validation.validate_exp = true;
        validation.leeway = 60;
        validation.set_required_spec_claims(&["exp", "iss", "aud"]);
        validation.set_issuer(&[issuer]);
        validation.set_audience(&[audience]);
        Self {
            decoding: DecodingKey::from_secret(secret.as_bytes()),
            validation,
        }
    }

    pub fn validate(&self, token: &str) -> Result<Claims, JwtValidationError> {
        jsonwebtoken::decode::<Claims>(token, &self.decoding, &self.validation)
            .map(|data| data.claims)
            .map_err(|_| JwtValidationError::Invalid)
    }
}
