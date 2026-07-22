use crate::crypto::CryptoService;
use crate::db::Db;
use crate::domain::bank_account::BankAccountId;
use crate::domain::category::CategoryId;
use crate::domain::compte::ProprietaireId;
use crate::domain::ports::ecriture::{
    BankTransactionsWriteRepository, EcritureError, ResultatInsertion,
};
use crate::domain::ports::lecture::{
    FiltreTransactions, LectureError, LectureResultat, ReglesCategorisationReadRepository, Tranche,
    TransactionsBancairesReadRepository,
};
use crate::domain::regle_categorisation::{RegleCategorisation, selectionner_regle};
use crate::domain::transaction_bancaire::{
    CategorisationTransaction, CategorizationSource, NouvelleTransactionBancaire,
    TransactionBancaire, TransactionBancaireId, TransactionStatus,
};
use crate::repository::chiffrement::{
    ChiffrementError, KEY_VERSION, chiffrer_montant, chiffrer_texte, dechiffrer_montant,
    dechiffrer_texte, vers_ecriture_error,
};
use crate::repository::regles_categorisation::SqlxReglesCategorisationRepository;
use chrono::{DateTime, NaiveDate, Utc};
use std::sync::Arc;
use uuid::Uuid;

pub(crate) const TABLE: &str = "bank_transaction";
const FIELD_EXTERNAL_TRANSACTION_ID: &str = "external_transaction_id";
const FIELD_LABEL: &str = "label";
pub(crate) const FIELD_AMOUNT: &str = "amount_cents";
const LIMITE_RETROACTIF: i64 = 5000;

fn dedup_key_transaction(
    crypto: &CryptoService,
    bank_account: &BankAccountId,
    external_transaction_id: &str,
) -> String {
    crypto.dedup_key(bank_account.0.as_bytes(), external_transaction_id)
}

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
        let dedup = dedup_key_transaction(
            crypto,
            &nouvelle.bank_account,
            &nouvelle.external_transaction_id,
        );

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
             t.label, t.amount_cents, t.currency, t.booking_date, t.value_date, \
             t.category_id, t.categorization_source, t.rule_id, t.created_at \
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

    pub async fn lister_par_compte(
        &self,
        crypto: &CryptoService,
        proprietaire: &ProprietaireId,
        compte: &BankAccountId,
        filtre: FiltreTransactions,
        tranche: Tranche,
    ) -> Result<LectureResultat<TransactionBancaire>, ChiffrementError> {
        let condition_categorisation = if filtre.non_categorisees {
            " AND t.category_id IS NULL"
        } else {
            ""
        };

        let total: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM budgy.bank_transaction t \
             JOIN budgy.bank_account a ON a.id = t.bank_account_id \
             WHERE t.bank_account_id = $1 AND a.owner_id = $2{condition_categorisation}"
        ))
        .bind(compte.0)
        .bind(&proprietaire.0)
        .fetch_one(&self.db)
        .await?;

        let rows = sqlx::query_as::<_, BankTransactionRow>(&format!(
            "SELECT t.id, t.bank_account_id, a.owner_id, t.external_transaction_id, t.status, \
             t.label, t.amount_cents, t.currency, t.booking_date, t.value_date, \
             t.category_id, t.categorization_source, t.rule_id, t.created_at \
             FROM budgy.bank_transaction t \
             JOIN budgy.bank_account a ON a.id = t.bank_account_id \
             WHERE t.bank_account_id = $1 AND a.owner_id = $2{condition_categorisation} \
             ORDER BY t.booking_date DESC NULLS FIRST, t.value_date DESC NULLS LAST, \
             t.created_at DESC \
             LIMIT $3 OFFSET $4"
        ))
        .bind(compte.0)
        .bind(&proprietaire.0)
        .bind(i64::from(tranche.limit))
        .bind(i64::from(tranche.offset))
        .fetch_all(&self.db)
        .await?;

        let elements = rows
            .into_iter()
            .map(|row| into_transaction(crypto, row))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(LectureResultat {
            elements,
            total: total.max(0) as u64,
        })
    }

    pub async fn categoriser(
        &self,
        crypto: &CryptoService,
        proprietaire: &ProprietaireId,
        compte: &BankAccountId,
        transaction: &TransactionBancaireId,
        category: &CategoryId,
    ) -> Result<CategorisationTransaction, ChiffrementError> {
        if !self.categorie_accessible(proprietaire, category).await? {
            return Ok(CategorisationTransaction::CategorieIntrouvable);
        }

        let mise_a_jour: Option<Uuid> = sqlx::query_scalar(
            "UPDATE budgy.bank_transaction AS t \
             SET category_id = $1, categorization_source = $2, rule_id = NULL \
             FROM budgy.bank_account AS a \
             WHERE t.bank_account_id = a.id \
             AND t.id = $3 AND t.bank_account_id = $4 AND a.owner_id = $5 \
             RETURNING t.id",
        )
        .bind(category.0)
        .bind(CategorizationSource::Manual.as_str())
        .bind(transaction.0)
        .bind(compte.0)
        .bind(&proprietaire.0)
        .fetch_optional(&self.db)
        .await?;

        if mise_a_jour.is_none() {
            return Ok(CategorisationTransaction::TransactionIntrouvable);
        }

        match self.fetch(crypto, transaction).await? {
            Some(transaction) => Ok(CategorisationTransaction::Categorisee(transaction)),
            None => Ok(CategorisationTransaction::TransactionIntrouvable),
        }
    }

    async fn appliquer_regle(
        &self,
        proprietaire: &ProprietaireId,
        transaction: &TransactionBancaireId,
        regle: &RegleCategorisation,
    ) -> Result<(), ChiffrementError> {
        sqlx::query(
            "UPDATE budgy.bank_transaction AS t \
             SET category_id = $1, categorization_source = $2, rule_id = $3 \
             FROM budgy.bank_account AS a \
             WHERE t.bank_account_id = a.id \
             AND t.id = $4 AND a.owner_id = $5 \
             AND t.categorization_source <> $6",
        )
        .bind(regle.category_id.0)
        .bind(CategorizationSource::Rule.as_str())
        .bind(regle.id.0)
        .bind(transaction.0)
        .bind(&proprietaire.0)
        .bind(CategorizationSource::Manual.as_str())
        .execute(&self.db)
        .await?;

        Ok(())
    }

    async fn lister_non_categorisees_pour_proprietaire(
        &self,
        crypto: &CryptoService,
        proprietaire: &ProprietaireId,
    ) -> Result<Vec<(TransactionBancaireId, String)>, ChiffrementError> {
        let rows: Vec<(Uuid, Vec<u8>)> = sqlx::query_as(
            "SELECT t.id, t.label \
             FROM budgy.bank_transaction t \
             JOIN budgy.bank_account a ON a.id = t.bank_account_id \
             WHERE a.owner_id = $1 AND t.category_id IS NULL \
             LIMIT $2",
        )
        .bind(&proprietaire.0)
        .bind(LIMITE_RETROACTIF)
        .fetch_all(&self.db)
        .await?;

        if rows.len() as i64 >= LIMITE_RETROACTIF {
            tracing::warn!(
                limite = LIMITE_RETROACTIF,
                "plafond de transactions non catégorisées atteint lors de l'application rétroactive"
            );
        }

        rows.into_iter()
            .map(|(id, label_blob)| {
                let label =
                    dechiffrer_texte(crypto, &proprietaire.0, TABLE, FIELD_LABEL, &label_blob)?;
                Ok((TransactionBancaireId(id), label))
            })
            .collect()
    }

    async fn appliquer_regle_par_lot(
        &self,
        regle: &RegleCategorisation,
        transactions: &[Uuid],
    ) -> Result<u64, ChiffrementError> {
        let touchees = sqlx::query(
            "UPDATE budgy.bank_transaction AS t \
             SET category_id = $1, categorization_source = $2, rule_id = $3 \
             FROM budgy.bank_account AS a \
             WHERE t.bank_account_id = a.id \
             AND t.id = ANY($4) AND a.owner_id = $5 \
             AND t.categorization_source = $6",
        )
        .bind(regle.category_id.0)
        .bind(CategorizationSource::Rule.as_str())
        .bind(regle.id.0)
        .bind(transactions)
        .bind(&regle.owner_id.0)
        .bind(CategorizationSource::None.as_str())
        .execute(&self.db)
        .await?
        .rows_affected();

        Ok(touchees)
    }

    async fn categorie_accessible(
        &self,
        proprietaire: &ProprietaireId,
        category: &CategoryId,
    ) -> Result<bool, ChiffrementError> {
        let existe: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM budgy.category \
             WHERE id = $1 AND (owner_id IS NULL OR owner_id = $2)",
        )
        .bind(category.0)
        .bind(&proprietaire.0)
        .fetch_optional(&self.db)
        .await?;

        Ok(existe.is_some())
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
    regles: SqlxReglesCategorisationRepository,
    crypto: Arc<CryptoService>,
}

impl SqlxBankTransactionsWriteAdapter {
    pub fn new(db: Db, crypto: Arc<CryptoService>) -> Self {
        Self {
            repo: SqlxBankTransactionsRepository::new(db.clone()),
            regles: SqlxReglesCategorisationRepository::new(db),
            crypto,
        }
    }

    pub async fn categoriser(
        &self,
        proprietaire: &ProprietaireId,
        compte: &BankAccountId,
        transaction: &TransactionBancaireId,
        category: &CategoryId,
    ) -> Result<CategorisationTransaction, EcritureError> {
        self.repo
            .categoriser(&self.crypto, proprietaire, compte, transaction, category)
            .await
            .map_err(vers_ecriture_error)
    }

    pub async fn appliquer_regle_retroactif(
        &self,
        regle: &RegleCategorisation,
    ) -> Result<u64, EcritureError> {
        let candidats = self
            .repo
            .lister_non_categorisees_pour_proprietaire(&self.crypto, &regle.owner_id)
            .await
            .map_err(vers_ecriture_error)?;

        let cibles: Vec<Uuid> = candidats
            .into_iter()
            .filter(|(_, label)| regle.correspond(label))
            .map(|(id, _)| id.0)
            .collect();

        if cibles.is_empty() {
            return Ok(0);
        }

        self.repo
            .appliquer_regle_par_lot(regle, &cibles)
            .await
            .map_err(vers_ecriture_error)
    }

    async fn appliquer_regles_apres_insertion(
        &self,
        bank_account: &BankAccountId,
        transaction: &TransactionBancaireId,
        label: &str,
    ) {
        if let Err(erreur) = self
            .categoriser_transaction_inseree(bank_account, transaction, label)
            .await
        {
            tracing::warn!(
                erreur = %erreur,
                "application automatique des règles ignorée pour la transaction insérée"
            );
        }
    }

    async fn categoriser_transaction_inseree(
        &self,
        bank_account: &BankAccountId,
        transaction: &TransactionBancaireId,
        label: &str,
    ) -> Result<(), EcritureError> {
        let proprietaire = ProprietaireId(
            self.repo
                .owner_du_compte(bank_account)
                .await
                .map_err(vers_ecriture_error)?,
        );

        let regles = self
            .regles
            .lister_pour_proprietaire(&proprietaire)
            .await
            .map_err(|e| EcritureError::Acces(e.to_string()))?;

        if let Some(regle) = selectionner_regle(label, &regles) {
            self.repo
                .appliquer_regle(&proprietaire, transaction, regle)
                .await
                .map_err(vers_ecriture_error)?;
        }

        Ok(())
    }
}

impl BankTransactionsWriteRepository for SqlxBankTransactionsWriteAdapter {
    async fn enregistrer(
        &self,
        nouvelle: NouvelleTransactionBancaire,
    ) -> Result<ResultatInsertion<TransactionBancaireId>, EcritureError> {
        let bank_account = nouvelle.bank_account.clone();
        let label = nouvelle.label.clone();

        let resultat = self
            .repo
            .insert(&self.crypto, nouvelle)
            .await
            .map_err(vers_ecriture_error)?;

        if let ResultatInsertion::Inseree(ref id) = resultat {
            self.appliquer_regles_apres_insertion(&bank_account, id, &label)
                .await;
        }

        Ok(resultat)
    }
}

impl TransactionsBancairesReadRepository for SqlxBankTransactionsWriteAdapter {
    async fn lister_par_compte(
        &self,
        proprietaire: &ProprietaireId,
        compte: &BankAccountId,
        filtre: FiltreTransactions,
        tranche: Tranche,
    ) -> Result<LectureResultat<TransactionBancaire>, LectureError> {
        self.repo
            .lister_par_compte(&self.crypto, proprietaire, compte, filtre, tranche)
            .await
            .map_err(|e| LectureError::Acces(e.to_string()))
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
    Option<Uuid>,
    String,
    Option<Uuid>,
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
        category_id,
        categorization_source,
        rule_id,
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
    let categorization_source = CategorizationSource::parse(&categorization_source)
        .ok_or_else(|| ChiffrementError::UnknownEnum(categorization_source.clone()))?;

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
        category: category_id.map(CategoryId),
        categorization_source,
        rule_id,
        created_at,
    })
}
