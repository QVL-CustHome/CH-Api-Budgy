use crate::crypto::CryptoService;
use crate::db::Db;
use crate::domain::bank_account::BankAccountId;
use crate::domain::category::CategoryId;
use crate::domain::compte::ProprietaireId;
use crate::domain::libelle::extraire_tiers;
use crate::domain::ports::ecriture::{
    BankTransactionsWriteRepository, EcritureError, ResultatInsertion,
};
use crate::domain::ports::lecture::{
    FiltreTransactions, FiltreTransactionsProprietaire, LectureError, LectureResultat,
    RecurrentsReadRepository, ReglesCategorisationReadRepository, Tranche,
    TransactionsBancairesReadRepository,
};
use crate::domain::recurrence::OccurrenceRecurrente;
use crate::domain::recurrence::{OccurrenceTransaction, RecurrenceInterval, detecter_recurrences};
use crate::domain::regle_categorisation::{RegleCategorisation, selectionner_regle};
use crate::domain::transaction_bancaire::{
    CategorisationTransaction, CategorizationSource, ChampTriTransaction,
    NouvelleTransactionBancaire, OrdreTri, SensTransaction, TransactionBancaire,
    TransactionBancaireId, TransactionStatus, TriTransactions,
};
use crate::domain::transfert_interne::{
    CATEGORIE_VIREMENTS_INTERNES, MouvementCandidat, detecter_transferts_internes,
};
use crate::repository::chiffrement::{
    ChiffrementError, KEY_VERSION, chiffrer_montant, chiffrer_texte, dechiffrer_montant,
    dechiffrer_texte, vers_ecriture_error,
};
use crate::repository::regles_categorisation::SqlxReglesCategorisationRepository;
use chrono::{DateTime, NaiveDate, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

pub(crate) const TABLE: &str = "bank_transaction";
const FIELD_EXTERNAL_TRANSACTION_ID: &str = "external_transaction_id";
const FIELD_LABEL: &str = "label";
pub(crate) const FIELD_AMOUNT: &str = "amount_cents";
const LIMITE_RETROACTIF: i64 = 5000;

type LigneOccurrenceChiffree = (Uuid, Vec<u8>, Vec<u8>, Option<NaiveDate>, Option<NaiveDate>);
type LigneMouvementChiffree = (
    Uuid,
    Uuid,
    Vec<u8>,
    Option<NaiveDate>,
    Option<NaiveDate>,
    Vec<u8>,
    Option<Uuid>,
    String,
);
type LigneRecurrenteChiffree = (
    Option<Uuid>,
    Vec<u8>,
    Vec<u8>,
    Option<NaiveDate>,
    Option<NaiveDate>,
);

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
             t.category_id, t.enveloppe_id, t.categorization_source, t.rule_id, t.is_recurrent, \
             t.recurrence_interval, t.created_at \
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
             t.category_id, t.enveloppe_id, t.categorization_source, t.rule_id, t.is_recurrent, \
             t.recurrence_interval, t.created_at \
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

    pub async fn lister_pour_proprietaire(
        &self,
        crypto: &CryptoService,
        proprietaire: &ProprietaireId,
        filtre: FiltreTransactionsProprietaire,
        tri: TriTransactions,
        tranche: Tranche,
    ) -> Result<LectureResultat<TransactionBancaire>, ChiffrementError> {
        let compte = filtre.compte.as_ref().map(|c| c.0);
        let categorie = filtre.categorie.as_ref().map(|c| c.0);

        let rows = sqlx::query_as::<_, BankTransactionRow>(
            "SELECT t.id, t.bank_account_id, a.owner_id, t.external_transaction_id, t.status, \
             t.label, t.amount_cents, t.currency, t.booking_date, t.value_date, \
             t.category_id, t.enveloppe_id, t.categorization_source, t.rule_id, t.is_recurrent, \
             t.recurrence_interval, t.created_at \
             FROM budgy.bank_transaction t \
             JOIN budgy.bank_account a ON a.id = t.bank_account_id \
             WHERE a.owner_id = $1 \
             AND ($2::uuid IS NULL OR t.bank_account_id = $2) \
             AND ($3::uuid IS NULL OR t.category_id = $3) \
             AND ($4::date IS NULL OR COALESCE(t.booking_date, t.value_date) >= $4) \
             AND ($5::date IS NULL OR COALESCE(t.booking_date, t.value_date) <= $5)",
        )
        .bind(&proprietaire.0)
        .bind(compte)
        .bind(categorie)
        .bind(filtre.debut)
        .bind(filtre.fin)
        .fetch_all(&self.db)
        .await?;

        let transactions = rows
            .into_iter()
            .map(|row| into_transaction(crypto, row))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(filtrer_trier_paginer(
            transactions,
            filtre.sens,
            tri,
            tranche,
        ))
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

        // Ranger la transaction dans « Virements internes » l'exclut des calculs ;
        // l'en sortir la réintègre. C'est le pendant manuel de l'appariement auto.
        let mise_a_jour: Option<Uuid> = sqlx::query_scalar(
            "UPDATE budgy.bank_transaction AS t \
             SET category_id = $1, categorization_source = $2, rule_id = NULL, \
             is_internal_transfer = ($1 = $6) \
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
        .bind(CATEGORIE_VIREMENTS_INTERNES)
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
             AND NOT t.is_internal_transfer \
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

    /// Catégorie « Salaire » (revenu) vers laquelle les crédits sont catégorisés par
    /// défaut. Préfère celle du propriétaire, sinon la catégorie globale seedée.
    pub async fn categorie_credit_par_defaut(
        &self,
        owner: &str,
    ) -> Result<Option<Uuid>, ChiffrementError> {
        let id: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM budgy.category \
             WHERE name = 'Salaire' AND kind = 'revenu' \
             AND (coalesce(owner_id, '') = $1 OR coalesce(owner_id, '') = '') \
             ORDER BY (coalesce(owner_id, '') = $1) DESC \
             LIMIT 1",
        )
        .bind(owner)
        .fetch_optional(&self.db)
        .await?;
        Ok(id)
    }

    /// Ids des transactions non catégorisées (source `none`) qui sont des crédits
    /// (montant >= 0). Le montant étant chiffré, le filtre se fait après déchiffrement.
    async fn lister_credits_non_categorises(
        &self,
        crypto: &CryptoService,
        owner: &str,
    ) -> Result<Vec<Uuid>, ChiffrementError> {
        let rows: Vec<(Uuid, Vec<u8>)> = sqlx::query_as(
            "SELECT t.id, t.amount_cents \
             FROM budgy.bank_transaction t \
             JOIN budgy.bank_account a ON a.id = t.bank_account_id \
             WHERE a.owner_id = $1 AND t.categorization_source = 'none' \
             AND NOT t.is_internal_transfer \
             LIMIT $2",
        )
        .bind(owner)
        .bind(LIMITE_RETROACTIF)
        .fetch_all(&self.db)
        .await?;

        let mut credits = Vec::new();
        for (id, amount_blob) in rows {
            let amount = dechiffrer_montant(crypto, owner, TABLE, FIELD_AMOUNT, &amount_blob)?;
            if amount >= 0 {
                credits.push(id);
            }
        }
        Ok(credits)
    }

    /// Applique une catégorie par défaut (sans règle) à un lot de transactions,
    /// uniquement celles encore non catégorisées (`none`) : n'écrase jamais un
    /// choix manuel ni une règle de libellé.
    async fn appliquer_categorie_defaut_par_lot(
        &self,
        owner: &str,
        category: Uuid,
        ids: &[Uuid],
    ) -> Result<u64, ChiffrementError> {
        if ids.is_empty() {
            return Ok(0);
        }
        let touchees = sqlx::query(
            "UPDATE budgy.bank_transaction AS t \
             SET category_id = $1, categorization_source = $2, rule_id = NULL \
             FROM budgy.bank_account AS a \
             WHERE t.bank_account_id = a.id \
             AND t.id = ANY($3) AND a.owner_id = $4 \
             AND t.categorization_source = $5",
        )
        .bind(category)
        .bind(CategorizationSource::Rule.as_str())
        .bind(ids)
        .bind(owner)
        .bind(CategorizationSource::None.as_str())
        .execute(&self.db)
        .await?
        .rows_affected();
        Ok(touchees)
    }

    /// Libellé déchiffré d'une transaction, restreint au propriétaire + compte.
    async fn libelle_transaction(
        &self,
        crypto: &CryptoService,
        owner: &str,
        account: &BankAccountId,
        transaction: &TransactionBancaireId,
    ) -> Result<Option<String>, ChiffrementError> {
        let row: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT t.label FROM budgy.bank_transaction t \
             JOIN budgy.bank_account a ON a.id = t.bank_account_id \
             WHERE t.id = $1 AND t.bank_account_id = $2 AND a.owner_id = $3",
        )
        .bind(transaction.0)
        .bind(account.0)
        .bind(owner)
        .fetch_optional(&self.db)
        .await?;
        match row {
            Some(label_blob) => Ok(Some(dechiffrer_texte(
                crypto,
                owner,
                TABLE,
                FIELD_LABEL,
                &label_blob,
            )?)),
            None => Ok(None),
        }
    }

    pub async fn recalculer_recurrences(
        &self,
        crypto: &CryptoService,
        proprietaire: &ProprietaireId,
    ) -> Result<u64, ChiffrementError> {
        let occurrences = self
            .lister_occurrences_pour_detection(crypto, proprietaire)
            .await?;
        let recurrentes = detecter_recurrences(&occurrences);

        self.reinitialiser_recurrences(proprietaire).await?;

        let mut par_interval: HashMap<RecurrenceInterval, Vec<Uuid>> = HashMap::new();
        for recurrente in recurrentes {
            par_interval
                .entry(recurrente.interval)
                .or_default()
                .push(recurrente.id.0);
        }

        let mut marquees = 0;
        for (interval, ids) in par_interval {
            marquees += self
                .marquer_recurrentes(proprietaire, interval, &ids)
                .await?;
        }
        Ok(marquees)
    }

    /// Réapparie les virements internes du propriétaire et met à jour le
    /// marquage. Les transactions rangées manuellement dans la catégorie
    /// « Virements internes » conservent leur marquage.
    pub async fn recalculer_transferts_internes(
        &self,
        crypto: &CryptoService,
        proprietaire: &ProprietaireId,
    ) -> Result<u64, ChiffrementError> {
        let mouvements = self
            .lister_mouvements_pour_appariement(crypto, proprietaire)
            .await?;
        let apparies: Vec<Uuid> = detecter_transferts_internes(&mouvements)
            .into_iter()
            .map(|id| id.0)
            .collect();

        self.reinitialiser_transferts_internes(proprietaire).await?;
        self.marquer_transferts_internes(proprietaire, &apparies)
            .await
    }

    async fn lister_mouvements_pour_appariement(
        &self,
        crypto: &CryptoService,
        proprietaire: &ProprietaireId,
    ) -> Result<Vec<MouvementCandidat>, ChiffrementError> {
        let rows: Vec<LigneMouvementChiffree> = sqlx::query_as(
            "SELECT t.id, t.bank_account_id, t.amount_cents, t.booking_date, t.value_date, \
             t.label, t.category_id, t.categorization_source \
             FROM budgy.bank_transaction t \
             JOIN budgy.bank_account a ON a.id = t.bank_account_id \
             WHERE a.owner_id = $1",
        )
        .bind(&proprietaire.0)
        .fetch_all(&self.db)
        .await?;

        let mut mouvements = Vec::with_capacity(rows.len());
        for (
            id,
            bank_account_id,
            amount_blob,
            booking_date,
            value_date,
            label_blob,
            category_id,
            source,
        ) in rows
        {
            let Some(date) = booking_date.or(value_date) else {
                continue;
            };
            let amount_cents =
                dechiffrer_montant(crypto, &proprietaire.0, TABLE, FIELD_AMOUNT, &amount_blob)?;
            let libelle_brut =
                dechiffrer_texte(crypto, &proprietaire.0, TABLE, FIELD_LABEL, &label_blob)?;
            // Un mouvement rangé à la main dans « Virements internes » n'est pas
            // un indice contre l'appariement : c'est l'inverse.
            let range_a_la_main = source == CategorizationSource::Manual.as_str()
                && category_id.is_some_and(|id| id != CATEGORIE_VIREMENTS_INTERNES);
            mouvements.push(MouvementCandidat {
                id: TransactionBancaireId(id),
                compte: BankAccountId(bank_account_id),
                amount_cents,
                date,
                libelle: extraire_tiers(&libelle_brut),
                range_a_la_main,
            });
        }
        Ok(mouvements)
    }

    /// Identifiants des mouvements exclus des calculs parce qu'appariés comme
    /// virements internes. Requête à part plutôt qu'un champ de plus sur
    /// `TransactionBancaire` : c'est un attribut d'affichage, pas du domaine.
    pub async fn ids_transferts_internes(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<std::collections::HashSet<Uuid>, ChiffrementError> {
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT t.id FROM budgy.bank_transaction t              JOIN budgy.bank_account a ON a.id = t.bank_account_id              WHERE a.owner_id = $1 AND t.is_internal_transfer",
        )
        .bind(&proprietaire.0)
        .fetch_all(&self.db)
        .await?;
        Ok(ids.into_iter().collect())
    }

    /// Efface le marquage automatique, en préservant les transactions rangées
    /// manuellement dans la catégorie « Virements internes ».
    async fn reinitialiser_transferts_internes(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<(), ChiffrementError> {
        sqlx::query(
            "UPDATE budgy.bank_transaction AS t \
             SET is_internal_transfer = false \
             FROM budgy.bank_account AS a \
             WHERE t.bank_account_id = a.id AND a.owner_id = $1 \
             AND t.is_internal_transfer \
             AND t.category_id IS DISTINCT FROM $2",
        )
        .bind(&proprietaire.0)
        .bind(CATEGORIE_VIREMENTS_INTERNES)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    async fn marquer_transferts_internes(
        &self,
        proprietaire: &ProprietaireId,
        ids: &[Uuid],
    ) -> Result<u64, ChiffrementError> {
        if ids.is_empty() {
            return Ok(0);
        }
        // Le marquage retire aussi la catégorisation **automatique** héritée (un
        // virement pris pour un salaire, par exemple) : elle serait trompeuse à
        // l'affichage. Un choix manuel de l'utilisateur, lui, est conservé.
        let marquees = sqlx::query(
            "UPDATE budgy.bank_transaction AS t \
             SET is_internal_transfer = true, \
             category_id = CASE WHEN t.categorization_source = 'manual' \
             THEN t.category_id ELSE NULL END, \
             rule_id = CASE WHEN t.categorization_source = 'manual' \
             THEN t.rule_id ELSE NULL END, \
             categorization_source = CASE WHEN t.categorization_source = 'manual' \
             THEN t.categorization_source ELSE 'none' END \
             FROM budgy.bank_account AS a \
             WHERE t.bank_account_id = a.id AND a.owner_id = $1 \
             AND t.id = ANY($2) AND NOT t.is_internal_transfer",
        )
        .bind(&proprietaire.0)
        .bind(ids)
        .execute(&self.db)
        .await?
        .rows_affected();
        Ok(marquees)
    }

    async fn lister_occurrences_pour_detection(
        &self,
        crypto: &CryptoService,
        proprietaire: &ProprietaireId,
    ) -> Result<Vec<OccurrenceTransaction>, ChiffrementError> {
        let rows: Vec<LigneOccurrenceChiffree> = sqlx::query_as(
            "SELECT t.id, t.label, t.amount_cents, t.booking_date, t.value_date \
                 FROM budgy.bank_transaction t \
                 JOIN budgy.bank_account a ON a.id = t.bank_account_id \
                 WHERE a.owner_id = $1 AND NOT t.is_internal_transfer",
        )
        .bind(&proprietaire.0)
        .fetch_all(&self.db)
        .await?;

        let mut occurrences = Vec::with_capacity(rows.len());
        for (id, label_blob, amount_blob, booking_date, value_date) in rows {
            let Some(date) = booking_date.or(value_date) else {
                continue;
            };
            let label = dechiffrer_texte(crypto, &proprietaire.0, TABLE, FIELD_LABEL, &label_blob)?;
            let amount_cents =
                dechiffrer_montant(crypto, &proprietaire.0, TABLE, FIELD_AMOUNT, &amount_blob)?;
            occurrences.push(OccurrenceTransaction {
                id: TransactionBancaireId(id),
                label,
                amount_cents,
                date,
            });
        }
        Ok(occurrences)
    }

    async fn lister_recurrents_pour_proprietaire(
        &self,
        crypto: &CryptoService,
        proprietaire: &ProprietaireId,
    ) -> Result<Vec<OccurrenceRecurrente>, ChiffrementError> {
        let rows: Vec<LigneRecurrenteChiffree> = sqlx::query_as(
            "SELECT t.category_id, t.label, t.amount_cents, t.booking_date, t.value_date \
             FROM budgy.bank_transaction t \
             JOIN budgy.bank_account a ON a.id = t.bank_account_id \
             WHERE a.owner_id = $1 AND t.is_recurrent = true \
             AND NOT t.is_internal_transfer",
        )
        .bind(&proprietaire.0)
        .fetch_all(&self.db)
        .await?;

        let mut occurrences = Vec::with_capacity(rows.len());
        for (category_id, label_blob, amount_blob, booking_date, value_date) in rows {
            let Some(date) = booking_date.or(value_date) else {
                continue;
            };
            let label = dechiffrer_texte(crypto, &proprietaire.0, TABLE, FIELD_LABEL, &label_blob)?;
            let amount_cents =
                dechiffrer_montant(crypto, &proprietaire.0, TABLE, FIELD_AMOUNT, &amount_blob)?;
            occurrences.push(OccurrenceRecurrente {
                category_id: category_id.map(CategoryId),
                label,
                amount_cents,
                date,
            });
        }
        Ok(occurrences)
    }

    async fn reinitialiser_recurrences(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<(), ChiffrementError> {
        sqlx::query(
            "UPDATE budgy.bank_transaction AS t \
             SET is_recurrent = false, recurrence_interval = NULL \
             FROM budgy.bank_account AS a \
             WHERE t.bank_account_id = a.id AND a.owner_id = $1 AND t.is_recurrent = true",
        )
        .bind(&proprietaire.0)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    async fn marquer_recurrentes(
        &self,
        proprietaire: &ProprietaireId,
        interval: RecurrenceInterval,
        ids: &[Uuid],
    ) -> Result<u64, ChiffrementError> {
        if ids.is_empty() {
            return Ok(0);
        }

        let touchees = sqlx::query(
            "UPDATE budgy.bank_transaction AS t \
             SET is_recurrent = true, recurrence_interval = $1 \
             FROM budgy.bank_account AS a \
             WHERE t.bank_account_id = a.id AND t.id = ANY($2) AND a.owner_id = $3",
        )
        .bind(interval.as_str())
        .bind(ids)
        .bind(&proprietaire.0)
        .execute(&self.db)
        .await?
        .rows_affected();
        Ok(touchees)
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

    /// Voir [`SqlxBankTransactionsRepository::ids_transferts_internes`].
    pub async fn ids_transferts_internes(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<std::collections::HashSet<Uuid>, EcritureError> {
        self.repo
            .ids_transferts_internes(proprietaire)
            .await
            .map_err(vers_ecriture_error)
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

    /// Réapparie les virements internes (débit d'un compte ↔ crédit d'un autre)
    /// pour les exclure des dépenses comme des revenus. Renvoie le nombre de
    /// transactions nouvellement marquées.
    pub async fn recalculer_transferts_internes(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<u64, EcritureError> {
        self.repo
            .recalculer_transferts_internes(&self.crypto, proprietaire)
            .await
            .map_err(vers_ecriture_error)
    }

    /// Catégorise en « Salaire » tous les crédits encore non catégorisés du
    /// propriétaire (rattrapage / catégorisation automatique des revenus).
    /// N'écrase jamais une catégorisation manuelle ou par règle.
    pub async fn recategoriser_credits(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<u64, EcritureError> {
        let Some(category) = self
            .repo
            .categorie_credit_par_defaut(&proprietaire.0)
            .await
            .map_err(vers_ecriture_error)?
        else {
            return Ok(0);
        };
        let credits = self
            .repo
            .lister_credits_non_categorises(&self.crypto, &proprietaire.0)
            .await
            .map_err(vers_ecriture_error)?;
        self.repo
            .appliquer_categorie_defaut_par_lot(&proprietaire.0, category, &credits)
            .await
            .map_err(vers_ecriture_error)
    }

    /// Libellé déchiffré d'une transaction (propriétaire + compte), pour dériver
    /// automatiquement le motif d'une règle.
    pub async fn libelle_transaction(
        &self,
        proprietaire: &ProprietaireId,
        compte: &BankAccountId,
        transaction: &TransactionBancaireId,
    ) -> Result<Option<String>, EcritureError> {
        self.repo
            .libelle_transaction(&self.crypto, &proprietaire.0, compte, transaction)
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

    async fn recalculer_recurrences(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<u64, EcritureError> {
        self.repo
            .recalculer_recurrences(&self.crypto, proprietaire)
            .await
            .map_err(vers_ecriture_error)
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

    async fn lister_pour_proprietaire(
        &self,
        proprietaire: &ProprietaireId,
        filtre: FiltreTransactionsProprietaire,
        tri: TriTransactions,
        tranche: Tranche,
    ) -> Result<LectureResultat<TransactionBancaire>, LectureError> {
        self.repo
            .lister_pour_proprietaire(&self.crypto, proprietaire, filtre, tri, tranche)
            .await
            .map_err(|e| LectureError::Acces(e.to_string()))
    }
}

impl RecurrentsReadRepository for SqlxBankTransactionsWriteAdapter {
    async fn lister_recurrents_pour_proprietaire(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<Vec<OccurrenceRecurrente>, LectureError> {
        self.repo
            .lister_recurrents_pour_proprietaire(&self.crypto, proprietaire)
            .await
            .map_err(|e| LectureError::Acces(e.to_string()))
    }
}

fn filtrer_trier_paginer(
    transactions: Vec<TransactionBancaire>,
    sens: Option<SensTransaction>,
    tri: TriTransactions,
    tranche: Tranche,
) -> LectureResultat<TransactionBancaire> {
    let mut filtrees = match sens {
        Some(sens) => transactions
            .into_iter()
            .filter(|transaction| transaction.sens() == sens)
            .collect::<Vec<_>>(),
        None => transactions,
    };

    trier(&mut filtrees, tri);

    let total = filtrees.len() as u64;
    let elements = filtrees
        .into_iter()
        .skip(tranche.offset as usize)
        .take(tranche.limit as usize)
        .collect();

    LectureResultat { elements, total }
}

fn trier(transactions: &mut [TransactionBancaire], tri: TriTransactions) {
    transactions.sort_by(|gauche, droite| {
        let base = match tri.champ {
            ChampTriTransaction::Date => gauche.date_effective().cmp(&droite.date_effective()),
            ChampTriTransaction::Montant => gauche.amount_cents.cmp(&droite.amount_cents),
        }
        .then_with(|| gauche.created_at.cmp(&droite.created_at))
        .then_with(|| gauche.id.0.cmp(&droite.id.0));

        match tri.ordre {
            OrdreTri::Ascendant => base,
            OrdreTri::Descendant => base.reverse(),
        }
    });
}

/// sqlx ne dérive `FromRow` que jusqu'à seize colonnes : au-delà, il faut une
/// structure nommée. L'ordre des champs doit suivre celui du SELECT.
#[derive(sqlx::FromRow)]
struct BankTransactionRow {
    id: Uuid,
    bank_account_id: Uuid,
    owner_id: String,
    external_transaction_id: Vec<u8>,
    status: String,
    label: Vec<u8>,
    amount_cents: Vec<u8>,
    currency: String,
    booking_date: Option<NaiveDate>,
    value_date: Option<NaiveDate>,
    category_id: Option<Uuid>,
    enveloppe_id: Option<Uuid>,
    categorization_source: String,
    rule_id: Option<Uuid>,
    is_recurrent: bool,
    recurrence_interval: Option<String>,
    created_at: DateTime<Utc>,
}

fn into_transaction(
    crypto: &CryptoService,
    row: BankTransactionRow,
) -> Result<TransactionBancaire, ChiffrementError> {
    let BankTransactionRow {
        id,
        bank_account_id,
        owner_id,
        external_transaction_id: external_transaction_id_blob,
        status,
        label: label_blob,
        amount_cents: amount_blob,
        currency,
        booking_date,
        value_date,
        category_id,
        enveloppe_id,
        categorization_source,
        rule_id,
        is_recurrent,
        recurrence_interval,
        created_at,
    } = row;

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
    let recurrence_interval = match recurrence_interval {
        Some(value) => Some(
            RecurrenceInterval::parse(&value)
                .ok_or_else(|| ChiffrementError::UnknownEnum(value.clone()))?,
        ),
        None => None,
    };

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
        enveloppe: enveloppe_id,
        categorization_source,
        rule_id,
        is_recurrent,
        recurrence_interval,
        created_at,
    })
}
