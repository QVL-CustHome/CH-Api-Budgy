use crate::crypto::CryptoService;
use crate::db::Db;
use crate::domain::bank_account::{BankAccount, BankAccountId, NouveauBankAccount, masquer_iban};
use crate::domain::compte::ProprietaireId;
use crate::domain::consent::ConsentId;
use crate::domain::ports::ecriture::{BankAccountsWriteRepository, EcritureError};
use crate::repository::chiffrement::{
    ChiffrementError, KEY_VERSION, chiffrer_texte, dechiffrer_texte, vers_ecriture_error,
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

const TABLE: &str = "bank_account";
const FIELD_EXTERNAL_ACCOUNT_ID: &str = "external_account_id";
const FIELD_IBAN: &str = "iban";

#[derive(Clone)]
pub struct SqlxBankAccountsRepository {
    db: Db,
}

impl SqlxBankAccountsRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub async fn insert(
        &self,
        crypto: &CryptoService,
        nouveau: NouveauBankAccount,
    ) -> Result<BankAccountId, ChiffrementError> {
        let owner = &nouveau.proprietaire.0;
        let external_account_id = chiffrer_texte(
            crypto,
            owner,
            TABLE,
            FIELD_EXTERNAL_ACCOUNT_ID,
            &nouveau.external_account_id,
        )?;
        let iban_encrypted = chiffrer_texte(crypto, owner, TABLE, FIELD_IBAN, &nouveau.iban)?;
        let iban_masked = masquer_iban(&nouveau.iban);

        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO budgy.bank_account \
             (owner_id, consent_id, external_account_id, iban_encrypted, iban_masked, currency, next_sync_at, sync_count_today, key_version) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, 0, $8) RETURNING id",
        )
        .bind(owner)
        .bind(nouveau.consent.0)
        .bind(external_account_id)
        .bind(iban_encrypted)
        .bind(iban_masked)
        .bind(&nouveau.currency)
        .bind(nouveau.next_sync_at)
        .bind(KEY_VERSION)
        .fetch_one(&self.db)
        .await?;

        Ok(BankAccountId(id))
    }

    pub async fn fetch(
        &self,
        crypto: &CryptoService,
        id: &BankAccountId,
    ) -> Result<Option<BankAccount>, ChiffrementError> {
        let Some(row) = sqlx::query_as::<_, BankAccountRow>(
            "SELECT id, owner_id, consent_id, external_account_id, iban_masked, currency, \
             next_sync_at, sync_count_today, created_at, updated_at \
             FROM budgy.bank_account WHERE id = $1",
        )
        .bind(id.0)
        .fetch_optional(&self.db)
        .await?
        else {
            return Ok(None);
        };

        Ok(Some(into_bank_account(crypto, row)?))
    }
}

#[derive(Clone)]
pub struct SqlxBankAccountsWriteAdapter {
    repo: SqlxBankAccountsRepository,
    crypto: Arc<CryptoService>,
}

impl SqlxBankAccountsWriteAdapter {
    pub fn new(db: Db, crypto: Arc<CryptoService>) -> Self {
        Self {
            repo: SqlxBankAccountsRepository::new(db),
            crypto,
        }
    }
}

impl BankAccountsWriteRepository for SqlxBankAccountsWriteAdapter {
    async fn enregistrer(
        &self,
        nouveau: NouveauBankAccount,
    ) -> Result<BankAccountId, EcritureError> {
        self.repo
            .insert(&self.crypto, nouveau)
            .await
            .map_err(vers_ecriture_error)
    }
}

type BankAccountRow = (
    Uuid,
    String,
    Uuid,
    Vec<u8>,
    String,
    String,
    Option<DateTime<Utc>>,
    i32,
    DateTime<Utc>,
    DateTime<Utc>,
);

fn into_bank_account(
    crypto: &CryptoService,
    row: BankAccountRow,
) -> Result<BankAccount, ChiffrementError> {
    let (
        id,
        owner_id,
        consent_id,
        external_account_id_blob,
        iban_masked,
        currency,
        next_sync_at,
        sync_count_today,
        created_at,
        updated_at,
    ) = row;

    let external_account_id = dechiffrer_texte(
        crypto,
        &owner_id,
        TABLE,
        FIELD_EXTERNAL_ACCOUNT_ID,
        &external_account_id_blob,
    )?;

    Ok(BankAccount {
        id: BankAccountId(id),
        proprietaire: ProprietaireId(owner_id),
        consent: ConsentId(consent_id),
        external_account_id,
        iban_masked,
        currency,
        next_sync_at,
        sync_count_today,
        created_at,
        updated_at,
    })
}
