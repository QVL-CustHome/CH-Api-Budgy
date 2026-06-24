use crate::db::Db;
use crate::domain::categorie::CategorieId;
use crate::domain::compte::CompteId;
use crate::domain::ports::lecture::{
    LectureError, LectureResultat, ListeTransactionsQuery, TransactionsReadRepository,
};
use crate::domain::transaction::{SensTransaction, Transaction, TransactionId};
use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

#[derive(Clone)]
pub struct SqlxTransactionsRepository {
    db: Db,
}

impl SqlxTransactionsRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

type TransactionRow = (
    Uuid,
    Uuid,
    String,
    i64,
    String,
    String,
    NaiveDate,
    Option<NaiveDate>,
    Option<Uuid>,
    Option<String>,
    DateTime<Utc>,
);

const TRANSACTION_COLUMNS: &str = "id, account_id, label, amount_cents, direction, currency, \
     operation_date, value_date, category_id, external_reference, created_at";

fn sens_from(direction: &str) -> Result<SensTransaction, LectureError> {
    match direction {
        "debit" => Ok(SensTransaction::Debit),
        "credit" => Ok(SensTransaction::Credit),
        other => Err(LectureError::Acces(format!("sens de transaction inconnu : {other}"))),
    }
}

fn into_transaction(row: TransactionRow) -> Result<Transaction, LectureError> {
    Ok(Transaction {
        id: TransactionId(row.0),
        compte: CompteId(row.1),
        libelle: row.2,
        montant_centimes: row.3,
        sens: sens_from(&row.4)?,
        devise: row.5,
        date_operation: row.6,
        date_valeur: row.7,
        categorie: row.8.map(CategorieId),
        reference_externe: row.9,
        cree_le: row.10,
    })
}

impl TransactionsReadRepository for SqlxTransactionsRepository {
    async fn lister(
        &self,
        query: ListeTransactionsQuery,
    ) -> Result<LectureResultat<Transaction>, LectureError> {
        let account_id = query.compte.as_ref().map(|c| c.0);

        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM budgy.transaction \
             WHERE owner_id = $1 \
             AND ($2::uuid IS NULL OR account_id = $2) \
             AND ($3::date IS NULL OR operation_date >= $3) \
             AND ($4::date IS NULL OR operation_date <= $4)",
        )
        .bind(&query.owner.0)
        .bind(account_id)
        .bind(query.depuis)
        .bind(query.jusqua)
        .fetch_one(&self.db)
        .await
        .map_err(|e| LectureError::Acces(e.to_string()))?;

        let rows = sqlx::query_as::<_, TransactionRow>(&format!(
            "SELECT {TRANSACTION_COLUMNS} FROM budgy.transaction \
             WHERE owner_id = $1 \
             AND ($2::uuid IS NULL OR account_id = $2) \
             AND ($3::date IS NULL OR operation_date >= $3) \
             AND ($4::date IS NULL OR operation_date <= $4) \
             ORDER BY operation_date DESC, created_at DESC \
             LIMIT $5 OFFSET $6"
        ))
        .bind(&query.owner.0)
        .bind(account_id)
        .bind(query.depuis)
        .bind(query.jusqua)
        .bind(i64::from(query.tranche.limit))
        .bind(i64::from(query.tranche.offset))
        .fetch_all(&self.db)
        .await
        .map_err(|e| LectureError::Acces(e.to_string()))?;

        let elements = rows
            .into_iter()
            .map(into_transaction)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(LectureResultat {
            elements,
            total: total.max(0) as u64,
        })
    }
}
