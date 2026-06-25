use crate::crypto::CryptoService;
use crate::db::Db;
use crate::domain::bank_account::BankAccountId;
use crate::domain::ports::ecriture::{
    BankTransactionsWriteRepository, EcritureError, ResultatInsertion,
};
use crate::domain::transaction_bancaire::{
    NouvelleTransactionBancaire, TransactionBancaire, TransactionBancaireId, TransactionStatus,
    dedup_key,
};
use crate::repository::chiffrement::{
    ChiffrementError, KEY_VERSION, chiffrer_montant, chiffrer_texte, dechiffrer_montant,
    dechiffrer_texte, vers_ecriture_error,
};
use chrono::{DateTime, NaiveDate, Utc};
use std::sync::Arc;
use uuid::Uuid;

const TABLE: &str = "bank_transaction";
const FIELD_EXTERNAL_TRANSACTION_ID: &str = "external_transaction_id";
const FIELD_LABEL: &str = "label";
const FIELD_AMOUNT: &str = "amount_cents";

#[derive(Clone)]
pub struct SqlxBankTransactionsRepository {
    db: Db,
}

impl SqlxBankTransactionsRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub async fn insert(
        &self,
        crypto: &CryptoService,
        nouvelle: NouvelleTransactionBancaire,
    ) -> Result<ResultatInsertion<TransactionBancaireId>, ChiffrementError> {
        let owner = self.owner_du_compte(&nouvelle.bank_account).await?;
        let external_transaction_id = chiffrer_texte(
            crypto,
            &owner,
            TABLE,
            FIELD_EXTERNAL_TRANSACTION_ID,
            &nouvelle.external_transaction_id,
        )?;
        let label = chiffrer_texte(crypto, &owner, TABLE, FIELD_LABEL, &nouvelle.label)?;
        let amount = chiffrer_montant(crypto, &owner, TABLE, FIELD_AMOUNT, nouvelle.amount_cents)?;
        let dedup = dedup_key(&nouvelle.bank_account, &nouvelle.external_transaction_id);

        let resultat: Option<(Uuid, bool)> = sqlx::query_as(
            "INSERT INTO budgy.bank_transaction \
             (bank_account_id, external_transaction_id, dedup_key, status, label, amount_cents, currency, booking_date, value_date, key_version) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
             ON CONFLICT ON CONSTRAINT bank_transaction_dedup_key_unique DO UPDATE SET \
             status = EXCLUDED.status, \
             booking_date = EXCLUDED.booking_date, \
             value_date = EXCLUDED.value_date \
             WHERE budgy.bank_transaction.status = $11 AND EXCLUDED.status = $12 \
             RETURNING id, (xmax = 0) AS inseree",
        )
        .bind(nouvelle.bank_account.0)
        .bind(external_transaction_id)
        .bind(dedup)
        .bind(nouvelle.status.as_str())
        .bind(label)
        .bind(amount)
        .bind(&nouvelle.currency)
        .bind(nouvelle.booking_date)
        .bind(nouvelle.value_date)
        .bind(KEY_VERSION)
        .bind(TransactionStatus::Pending.as_str())
        .bind(TransactionStatus::Booked.as_str())
        .fetch_optional(&self.db)
        .await?;

        Ok(match resultat {
            Some((id, true)) => ResultatInsertion::Inseree(TransactionBancaireId(id)),
            _ => ResultatInsertion::Doublon,
        })
    }

    pub async fn fetch(
        &self,
        crypto: &CryptoService,
        id: &TransactionBancaireId,
    ) -> Result<Option<TransactionBancaire>, ChiffrementError> {
        let Some(row) = sqlx::query_as::<_, BankTransactionRow>(
            "SELECT t.id, t.bank_account_id, a.owner_id, t.external_transaction_id, t.status, \
             t.label, t.amount_cents, t.currency, t.booking_date, t.value_date, t.created_at \
             FROM budgy.bank_transaction t \
             JOIN budgy.bank_account a ON a.id = t.bank_account_id \
             WHERE t.id = $1",
        )
        .bind(id.0)
        .fetch_optional(&self.db)
        .await?
        else {
            return Ok(None);
        };

        Ok(Some(into_transaction(crypto, row)?))
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
pub struct SqlxBankTransactionsWriteAdapter {
    repo: SqlxBankTransactionsRepository,
    crypto: Arc<CryptoService>,
}

impl SqlxBankTransactionsWriteAdapter {
    pub fn new(db: Db, crypto: Arc<CryptoService>) -> Self {
        Self {
            repo: SqlxBankTransactionsRepository::new(db),
            crypto,
        }
    }
}

impl BankTransactionsWriteRepository for SqlxBankTransactionsWriteAdapter {
    async fn enregistrer(
        &self,
        nouvelle: NouvelleTransactionBancaire,
    ) -> Result<ResultatInsertion<TransactionBancaireId>, EcritureError> {
        self.repo
            .insert(&self.crypto, nouvelle)
            .await
            .map_err(vers_ecriture_error)
    }
}

type BankTransactionRow = (
    Uuid,
    Uuid,
    String,
    Vec<u8>,
    String,
    Vec<u8>,
    Vec<u8>,
    String,
    Option<NaiveDate>,
    Option<NaiveDate>,
    DateTime<Utc>,
);

fn into_transaction(
    crypto: &CryptoService,
    row: BankTransactionRow,
) -> Result<TransactionBancaire, ChiffrementError> {
    let (
        id,
        bank_account_id,
        owner_id,
        external_transaction_id_blob,
        status,
        label_blob,
        amount_blob,
        currency,
        booking_date,
        value_date,
        created_at,
    ) = row;

    let external_transaction_id = dechiffrer_texte(
        crypto,
        &owner_id,
        TABLE,
        FIELD_EXTERNAL_TRANSACTION_ID,
        &external_transaction_id_blob,
    )?;
    let label = dechiffrer_texte(crypto, &owner_id, TABLE, FIELD_LABEL, &label_blob)?;
    let amount_cents = dechiffrer_montant(crypto, &owner_id, TABLE, FIELD_AMOUNT, &amount_blob)?;
    let status = TransactionStatus::parse(&status)
        .ok_or_else(|| ChiffrementError::UnknownEnum(status.clone()))?;

    Ok(TransactionBancaire {
        id: TransactionBancaireId(id),
        bank_account: BankAccountId(bank_account_id),
        external_transaction_id,
        status,
        label,
        amount_cents,
        currency,
        booking_date,
        value_date,
        created_at,
    })
}
