use crate::crypto::CryptoService;
use crate::db::Db;
use crate::domain::balance::{Balance, BalanceId, BalanceType, NouvelleBalance};
use crate::domain::bank_account::BankAccountId;
use crate::domain::ports::ecriture::{BalancesWriteRepository, EcritureError};
use crate::repository::chiffrement::{
    ChiffrementError, KEY_VERSION, chiffrer_montant, dechiffrer_montant, vers_ecriture_error,
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

const TABLE: &str = "balance";
const FIELD_AMOUNT: &str = "amount_cents";

#[derive(Clone)]
pub struct SqlxBalancesRepository {
    db: Db,
}

impl SqlxBalancesRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub async fn insert(
        &self,
        crypto: &CryptoService,
        nouvelle: NouvelleBalance,
    ) -> Result<BalanceId, ChiffrementError> {
        let owner = self.owner_du_compte(&nouvelle.bank_account).await?;
        let amount = chiffrer_montant(crypto, &owner, TABLE, FIELD_AMOUNT, nouvelle.amount_cents)?;

        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO budgy.balance \
             (bank_account_id, balance_type, amount_cents, currency, reference_date, key_version) \
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
        )
        .bind(nouvelle.bank_account.0)
        .bind(nouvelle.balance_type.as_str())
        .bind(amount)
        .bind(&nouvelle.currency)
        .bind(nouvelle.reference_date)
        .bind(KEY_VERSION)
        .fetch_one(&self.db)
        .await?;

        Ok(BalanceId(id))
    }

    pub async fn fetch(
        &self,
        crypto: &CryptoService,
        id: &BalanceId,
    ) -> Result<Option<Balance>, ChiffrementError> {
        let Some(row) = sqlx::query_as::<_, BalanceRow>(
            "SELECT b.id, b.bank_account_id, a.owner_id, b.balance_type, b.amount_cents, \
             b.currency, b.reference_date, b.created_at \
             FROM budgy.balance b \
             JOIN budgy.bank_account a ON a.id = b.bank_account_id \
             WHERE b.id = $1",
        )
        .bind(id.0)
        .fetch_optional(&self.db)
        .await?
        else {
            return Ok(None);
        };

        Ok(Some(into_balance(crypto, row)?))
    }

    async fn owner_du_compte(
        &self,
        bank_account: &BankAccountId,
    ) -> Result<String, ChiffrementError> {
        let owner: String =
            sqlx::query_scalar("SELECT owner_id FROM budgy.bank_account WHERE id = $1")
                .bind(bank_account.0)
                .fetch_one(&self.db)
                .await?;
        Ok(owner)
    }
}

#[derive(Clone)]
pub struct SqlxBalancesWriteAdapter {
    repo: SqlxBalancesRepository,
    crypto: Arc<CryptoService>,
}

impl SqlxBalancesWriteAdapter {
    pub fn new(db: Db, crypto: Arc<CryptoService>) -> Self {
        Self {
            repo: SqlxBalancesRepository::new(db),
            crypto,
        }
    }
}

impl BalancesWriteRepository for SqlxBalancesWriteAdapter {
    async fn enregistrer(&self, nouvelle: NouvelleBalance) -> Result<BalanceId, EcritureError> {
        self.repo
            .insert(&self.crypto, nouvelle)
            .await
            .map_err(vers_ecriture_error)
    }
}

type BalanceRow = (
    Uuid,
    Uuid,
    String,
    String,
    Vec<u8>,
    String,
    DateTime<Utc>,
    DateTime<Utc>,
);

fn into_balance(crypto: &CryptoService, row: BalanceRow) -> Result<Balance, ChiffrementError> {
    let (
        id,
        bank_account_id,
        owner_id,
        balance_type,
        amount_blob,
        currency,
        reference_date,
        created_at,
    ) = row;

    let balance_type = BalanceType::parse(&balance_type)
        .ok_or_else(|| ChiffrementError::UnknownEnum(balance_type.clone()))?;
    let amount_cents = dechiffrer_montant(crypto, &owner_id, TABLE, FIELD_AMOUNT, &amount_blob)?;

    Ok(Balance {
        id: BalanceId(id),
        bank_account: BankAccountId(bank_account_id),
        balance_type,
        amount_cents,
        currency,
        reference_date,
        created_at,
    })
}
