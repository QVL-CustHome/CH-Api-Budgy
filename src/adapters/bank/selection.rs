use crate::adapters::bank::mock::MockBankDataSource;
use crate::adapters::bank::reel::EnableBankingBankDataSource;
use crate::config::EnableBankingConfig;
use crate::domain::ports::bank_data_source::BankDataSource;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceBancaire {
    #[default]
    Mock,
    EnableBanking,
}

pub fn construire_source(
    source: SourceBancaire,
    enable_banking: &EnableBankingConfig,
) -> Arc<dyn BankDataSource> {
    match source {
        SourceBancaire::Mock => Arc::new(MockBankDataSource::new()),
        SourceBancaire::EnableBanking => {
            Arc::new(EnableBankingBankDataSource::depuis_config(enable_banking))
        }
    }
}
