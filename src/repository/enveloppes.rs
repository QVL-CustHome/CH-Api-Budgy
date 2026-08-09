use crate::crypto::CryptoService;
use crate::db::Db;
use crate::domain::compte::ProprietaireId;
use crate::domain::enveloppe::{
    Enveloppe, EnveloppeId, MiseAJourEnveloppe, NouvelleEnveloppe, SuiviEnveloppe,
};
use crate::domain::ports::ecriture::EcritureError;
use crate::domain::ports::lecture::LectureError;
use crate::repository::chiffrement::dechiffrer_montant;
use chrono::{DateTime, Utc};
use sqlx::FromRow;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

const TABLE: &str = "bank_transaction";
const FIELD_AMOUNT: &str = "amount_cents";

#[derive(Clone)]
pub struct SqlxEnveloppesRepository {
    db: Db,
    crypto: Arc<CryptoService>,
}

#[derive(FromRow)]
struct EnveloppeRow {
    id: Uuid,
    owner_id: String,
    nom: String,
    icon: String,
    color: String,
    montant_cents: i64,
    created_at: DateTime<Utc>,
}

fn into_enveloppe(row: EnveloppeRow) -> Enveloppe {
    Enveloppe {
        id: EnveloppeId(row.id),
        proprietaire: ProprietaireId(row.owner_id),
        nom: row.nom,
        icon: row.icon,
        color: row.color,
        montant_cents: row.montant_cents,
        created_at: row.created_at,
    }
}

/// Ce qui a été dépensé dans une enveloppe.
struct Consommation {
    depense_cents: i64,
    nombre: i64,
}

impl SqlxEnveloppesRepository {
    pub fn new(db: Db, crypto: Arc<CryptoService>) -> Self {
        Self { db, crypto }
    }

    /// Les montants étant chiffrés en base, la somme se fait ici après
    /// déchiffrement — aucun SUM() n'est possible côté SQL.
    async fn consommations(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<HashMap<Uuid, Consommation>, LectureError> {
        let lignes: Vec<(Uuid, Vec<u8>)> = sqlx::query_as(
            "SELECT t.enveloppe_id, t.amount_cents \
             FROM budgy.bank_transaction t \
             JOIN budgy.bank_account a ON a.id = t.bank_account_id \
             WHERE a.owner_id = $1 AND t.enveloppe_id IS NOT NULL",
        )
        .bind(&proprietaire.0)
        .fetch_all(&self.db)
        .await
        .map_err(|e| LectureError::Acces(e.to_string()))?;

        let mut totaux: HashMap<Uuid, Consommation> = HashMap::new();
        for (enveloppe_id, montant_chiffre) in lignes {
            let montant = dechiffrer_montant(
                &self.crypto,
                &proprietaire.0,
                TABLE,
                FIELD_AMOUNT,
                &montant_chiffre,
            )
            .map_err(|e| LectureError::Acces(e.to_string()))?;

            let entree = totaux.entry(enveloppe_id).or_insert(Consommation {
                depense_cents: 0,
                nombre: 0,
            });
            // Somme nette : un remboursement rangé dans l'enveloppe la recrédite,
            // au lieu de gonfler la dépense comme le ferait une valeur absolue.
            entree.depense_cents += -montant;
            entree.nombre += 1;
        }
        Ok(totaux)
    }

    pub async fn lister(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<Vec<SuiviEnveloppe>, LectureError> {
        let rows = sqlx::query_as::<_, EnveloppeRow>(
            "SELECT id, owner_id, nom, icon, color, montant_cents, created_at \
             FROM budgy.enveloppe WHERE owner_id = $1 ORDER BY nom",
        )
        .bind(&proprietaire.0)
        .fetch_all(&self.db)
        .await
        .map_err(|e| LectureError::Acces(e.to_string()))?;

        let mut consommations = self.consommations(proprietaire).await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let enveloppe = into_enveloppe(row);
                let consommation = consommations.remove(&enveloppe.id.0);
                SuiviEnveloppe {
                    depense_cents: consommation.as_ref().map_or(0, |c| c.depense_cents),
                    nombre_transactions: consommation.as_ref().map_or(0, |c| c.nombre),
                    enveloppe,
                }
            })
            .collect())
    }

    pub async fn creer(&self, nouvelle: NouvelleEnveloppe) -> Result<Enveloppe, EcritureError> {
        let row = sqlx::query_as::<_, EnveloppeRow>(
            "INSERT INTO budgy.enveloppe (owner_id, nom, icon, color, montant_cents) \
             VALUES ($1, $2, $3, $4, $5) \
             RETURNING id, owner_id, nom, icon, color, montant_cents, created_at",
        )
        .bind(&nouvelle.proprietaire.0)
        .bind(nouvelle.nom.as_str())
        .bind(&nouvelle.icon)
        .bind(&nouvelle.color)
        .bind(nouvelle.montant.cents())
        .fetch_one(&self.db)
        .await
        .map_err(|e| EcritureError::Acces(e.to_string()))?;
        Ok(into_enveloppe(row))
    }

    pub async fn modifier(
        &self,
        proprietaire: &ProprietaireId,
        id: &EnveloppeId,
        mise_a_jour: MiseAJourEnveloppe,
    ) -> Result<Option<Enveloppe>, EcritureError> {
        let row = sqlx::query_as::<_, EnveloppeRow>(
            "UPDATE budgy.enveloppe \
             SET nom = $3, icon = $4, color = $5, montant_cents = $6, updated_at = now() \
             WHERE id = $1 AND owner_id = $2 \
             RETURNING id, owner_id, nom, icon, color, montant_cents, created_at",
        )
        .bind(id.0)
        .bind(&proprietaire.0)
        .bind(mise_a_jour.nom.as_str())
        .bind(&mise_a_jour.icon)
        .bind(&mise_a_jour.color)
        .bind(mise_a_jour.montant.cents())
        .fetch_optional(&self.db)
        .await
        .map_err(|e| EcritureError::Acces(e.to_string()))?;
        Ok(row.map(into_enveloppe))
    }

    pub async fn supprimer(
        &self,
        proprietaire: &ProprietaireId,
        id: &EnveloppeId,
    ) -> Result<bool, EcritureError> {
        let resultat = sqlx::query("DELETE FROM budgy.enveloppe WHERE id = $1 AND owner_id = $2")
            .bind(id.0)
            .bind(&proprietaire.0)
            .execute(&self.db)
            .await
            .map_err(|e| EcritureError::Acces(e.to_string()))?;
        Ok(resultat.rows_affected() > 0)
    }

    /// Range une transaction dans une enveloppe, ou l'en retire avec `None`.
    /// Le classement par catégorie n'est pas touché : les deux cohabitent.
    pub async fn affecter_transaction(
        &self,
        proprietaire: &ProprietaireId,
        transaction_id: Uuid,
        enveloppe: Option<&EnveloppeId>,
    ) -> Result<bool, EcritureError> {
        let resultat = sqlx::query(
            "UPDATE budgy.bank_transaction t \
             SET enveloppe_id = $3 \
             FROM budgy.bank_account a \
             WHERE t.bank_account_id = a.id \
             AND t.id = $1 AND a.owner_id = $2 \
             AND ($3::uuid IS NULL OR EXISTS ( \
                 SELECT 1 FROM budgy.enveloppe e \
                 WHERE e.id = $3 AND e.owner_id = $2))",
        )
        .bind(transaction_id)
        .bind(&proprietaire.0)
        .bind(enveloppe.map(|e| e.0))
        .execute(&self.db)
        .await
        .map_err(|e| EcritureError::Acces(e.to_string()))?;
        Ok(resultat.rows_affected() > 0)
    }
}
