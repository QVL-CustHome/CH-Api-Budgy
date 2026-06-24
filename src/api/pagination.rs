use crate::api::error::ApiError;
use serde::Deserialize;

pub const DEFAULT_LIMIT: u32 = 50;
pub const MAX_LIMIT: u32 = 200;

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct PaginationParams {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pagination {
    pub limit: u32,
    pub offset: u32,
}

impl PaginationParams {
    pub fn resolve(self) -> Result<Pagination, ApiError> {
        let limit = match self.limit {
            None => DEFAULT_LIMIT,
            Some(0) => return Err(ApiError::validation("limit doit être supérieur à 0")),
            Some(value) if value > MAX_LIMIT => {
                return Err(ApiError::validation(format!(
                    "limit ne peut pas dépasser {MAX_LIMIT}"
                )));
            }
            Some(value) => value,
        };
        let offset = self.offset.unwrap_or(0);
        Ok(Pagination { limit, offset })
    }
}

impl Pagination {
    pub fn limit_i64(self) -> i64 {
        i64::from(self.limit)
    }

    pub fn offset_i64(self) -> i64 {
        i64::from(self.offset)
    }
}
