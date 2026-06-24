use crate::api::error::ApiError;
use crate::api::pagination::{Pagination, PaginationParams};
use chrono::NaiveDate;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateRangeFilter {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

impl DateRangeFilter {
    pub fn validate(self) -> Result<Self, ApiError> {
        match (self.from, self.to) {
            (Some(from), Some(to)) if from > to => {
                Err(ApiError::validation("from doit être antérieur ou égal à to"))
            }
            _ => Ok(self),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct ListQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub account_id: Option<Uuid>,
}

impl ListQuery {
    pub fn pagination(&self) -> Result<Pagination, ApiError> {
        PaginationParams {
            limit: self.limit,
            offset: self.offset,
        }
        .resolve()
    }

    pub fn date_range(&self) -> Result<DateRangeFilter, ApiError> {
        DateRangeFilter {
            from: self.from,
            to: self.to,
        }
        .validate()
    }
}
