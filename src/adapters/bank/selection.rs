use crate::adapters::bank::mock::MockBankDataSource;
use crate::adapters::bank::reel::GoCardlessBankDataSource;
use crate::domain::ports::bank_data_source::BankDataSource;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceBancaire {
    #[default]
    Mock,
    Gocardless,
}

pub fn construire_source(source: SourceBancaire) -> Arc<dyn BankDataSource> {
    match source {
        SourceBancaire::Mock => Arc::new(MockBankDataSource::new()),
        SourceBancaire::Gocardless => Arc::new(GoCardlessBankDataSource::new()),
    }
}
