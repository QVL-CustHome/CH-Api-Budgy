use crate::crypto::CryptoService;
use crate::db::Db;
use crate::domain::bank_account::{
    BankAccount, BankAccountId, CompteASynchroniser, NouveauBankAccount, PlanificationSynchro,
    dedup_key, masquer_iban,
};
use crate::domain::compte::ProprietaireId;
use crate::domain::consent::{Consent, ConsentId, ConsentStatus};
use crate::domain::ports::ecriture::{
    BankAccountsWriteRepository, EcritureError, PlanificationSynchroWriteRepository,
};
use crate::domain::ports::lecture::{
    BankAccountsReadRepository, CompteEcheant, LectureError, PlanificationSynchroReadRepository,
};
use crate::repository::chiffrement::{
    ChiffrementError, KEY_VERSION, chiffrer_texte, dechiffrer_texte, vers_ecriture_error,
};
use chrono::{DateTime, NaiveDate, Utc};
use std::sync::Arc;
use uuid::Uuid;

const FIELD_CONSENT_EXTERNAL_REF: &str = "external_ref";

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
        let dedup = dedup_key(&nouveau.consent, &nouveau.external_account_id);

        let id: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO budgy.bank_account \
             (owner_id, consent_id, external_account_id, iban_encrypted, iban_masked, currency, next_sync_at, sync_count_today, key_version, dedup_key) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, 0, $8, $9) \
             ON CONFLICT ON CONSTRAINT bank_account_consent_dedup_unique DO NOTHING \
             RETURNING id",
        )
        .bind(owner)
        .bind(nouveau.consent.0)
        .bind(external_account_id)
        .bind(iban_encrypted)
        .bind(iban_masked)
        .bind(&nouveau.currency)
        .bind(nouveau.next_sync_at)
        .bind(KEY_VERSION)
        .bind(&dedup)
        .fetch_optional(&self.db)
        .await?;

        match id {
            Some(id) => Ok(BankAccountId(id)),
            None => self.fetch_id_par_dedup(&nouveau.consent.0, &dedup).await,
        }
    }

    async fn fetch_id_par_dedup(
        &self,
        consent_id: &Uuid,
        dedup: &str,
    ) -> Result<BankAccountId, ChiffrementError> {
        let id: Uuid = sqlx::query_scalar(
            "SELECT id FROM budgy.bank_account WHERE consent_id = $1 AND dedup_key = $2",
        )
        .bind(consent_id)
        .bind(dedup)
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

    pub async fn lister_par_consent(
        &self,
        crypto: &CryptoService,
        proprietaire: &ProprietaireId,
        consent: &ConsentId,
    ) -> Result<Vec<BankAccount>, ChiffrementError> {
        let rows = sqlx::query_as::<_, BankAccountRow>(
            "SELECT id, owner_id, consent_id, external_account_id, iban_masked, currency, \
             next_sync_at, sync_count_today, created_at, updated_at \
             FROM budgy.bank_account WHERE owner_id = $1 AND consent_id = $2 \
             ORDER BY created_at ASC",
        )
        .bind(&proprietaire.0)
        .bind(consent.0)
        .fetch_all(&self.db)
        .await?;

        rows.into_iter()
            .map(|row| into_bank_account(crypto, row))
            .collect()
    }

    pub async fn supprimer_par_proprietaire(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<u64, ChiffrementError> {
        let resultat = sqlx::query("DELETE FROM budgy.bank_account WHERE owner_id = $1")
            .bind(&proprietaire.0)
            .execute(&self.db)
            .await?;
        Ok(resultat.rows_affected())
    }

    pub async fn lister_comptes_echeants(
        &self,
        crypto: &CryptoService,
        maintenant: DateTime<Utc>,
        quota_journalier: i32,
    ) -> Result<Vec<CompteEcheant>, ChiffrementError> {
        let jour_courant = maintenant.date_naive();
        let rows = sqlx::query_as::<_, CompteEcheantRow>(
            "SELECT a.id, a.owner_id, a.consent_id, a.external_account_id, a.currency, \
             a.sync_count_today, a.last_sync_day, \
             c.external_ref, c.status, c.expires_at, c.created_at, c.updated_at \
             FROM budgy.bank_account a \
             JOIN budgy.consent c ON c.id = a.consent_id \
             WHERE (a.next_sync_at IS NULL OR a.next_sync_at <= $1) \
             AND c.status = $2 \
             AND (c.expires_at IS NULL OR c.expires_at > $1) \
             AND (CASE WHEN a.last_sync_day = $3 THEN a.sync_count_today ELSE 0 END) < $4 \
             ORDER BY a.next_sync_at ASC NULLS FIRST",
        )
        .bind(maintenant)
        .bind(ConsentStatus::Active.as_str())
        .bind(jour_courant)
        .bind(quota_journalier)
        .fetch_all(&self.db)
        .await?;

        rows.into_iter()
            .map(|row| into_compte_echeant(crypto, row))
            .collect()
    }

    pub async fn reserver_creneau(
        &self,
        compte: &BankAccountId,
        plan: PlanificationSynchro,
        quota_journalier: i32,
    ) -> Result<bool, ChiffrementError> {
        let resultat = sqlx::query(
            "UPDATE budgy.bank_account SET \
             next_sync_at = $2, \
             sync_count_today = CASE WHEN last_sync_day = $3 THEN sync_count_today + 1 ELSE 1 END, \
             last_sync_day = $3, \
             last_sync_at = $4, \
             updated_at = now() \
             WHERE id = $1 \
             AND (CASE WHEN last_sync_day = $3 THEN sync_count_today ELSE 0 END) < $5",
        )
        .bind(compte.0)
        .bind(plan.next_sync_at)
        .bind(plan.last_sync_day)
        .bind(plan.last_sync_at)
        .bind(quota_journalier)
        .execute(&self.db)
        .await?;

        Ok(resultat.rows_affected() == 1)
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

    async fn supprimer_par_proprietaire(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<u64, EcritureError> {
        self.repo
            .supprimer_par_proprietaire(proprietaire)
            .await
            .map_err(vers_ecriture_error)
    }
}

impl BankAccountsReadRepository for SqlxBankAccountsWriteAdapter {
    async fn lister_par_consent(
        &self,
        proprietaire: &ProprietaireId,
        consent: &ConsentId,
    ) -> Result<Vec<BankAccount>, LectureError> {
        self.repo
            .lister_par_consent(&self.crypto, proprietaire, consent)
            .await
            .map_err(|e| LectureError::Acces(e.to_string()))
    }
}

impl PlanificationSynchroReadRepository for SqlxBankAccountsWriteAdapter {
    async fn lister_comptes_echeants(
        &self,
        maintenant: DateTime<Utc>,
        quota_journalier: i32,
    ) -> Result<Vec<CompteEcheant>, LectureError> {
        self.repo
            .lister_comptes_echeants(&self.crypto, maintenant, quota_journalier)
            .await
            .map_err(|e| LectureError::Acces(e.to_string()))
    }
}

impl PlanificationSynchroWriteRepository for SqlxBankAccountsWriteAdapter {
    async fn reserver_creneau(
        &self,
        compte: &BankAccountId,
        plan: PlanificationSynchro,
        quota_journalier: i32,
    ) -> Result<bool, EcritureError> {
        self.repo
            .reserver_creneau(compte, plan, quota_journalier)
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

type CompteEcheantRow = (
    Uuid,
    String,
    Uuid,
    Vec<u8>,
    String,
    i32,
    Option<NaiveDate>,
    Vec<u8>,
    String,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
    DateTime<Utc>,
);

fn into_compte_echeant(
    crypto: &CryptoService,
    row: CompteEcheantRow,
) -> Result<CompteEcheant, ChiffrementError> {
    let (
        account_id,
        owner_id,
        consent_id,
        external_account_id_blob,
        currency,
        sync_count_today,
        last_sync_day,
        consent_external_ref_blob,
        consent_status,
        consent_expires_at,
        consent_created_at,
        consent_updated_at,
    ) = row;

    let external_account_id = dechiffrer_texte(
        crypto,
        &owner_id,
        TABLE,
        FIELD_EXTERNAL_ACCOUNT_ID,
        &external_account_id_blob,
    )?;
    let consent_external_ref = dechiffrer_texte(
        crypto,
        &owner_id,
        "consent",
        FIELD_CONSENT_EXTERNAL_REF,
        &consent_external_ref_blob,
    )?;
    let status = ConsentStatus::parse(&consent_status)
        .ok_or_else(|| ChiffrementError::UnknownEnum(consent_status.clone()))?;

    let compte = CompteASynchroniser {
        id: BankAccountId(account_id),
        proprietaire: ProprietaireId(owner_id.clone()),
        consent: ConsentId(consent_id),
        external_account_id,
        currency,
        sync_count_today,
        last_sync_day,
    };
    let consent = Consent {
        id: ConsentId(consent_id),
        proprietaire: ProprietaireId(owner_id),
        external_ref: consent_external_ref,
        status,
        expires_at: consent_expires_at,
        created_at: consent_created_at,
        updated_at: consent_updated_at,
    };

    Ok(CompteEcheant { compte, consent })
}
