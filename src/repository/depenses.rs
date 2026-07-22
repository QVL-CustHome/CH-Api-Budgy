use crate::crypto::CryptoService;
use crate::db::Db;
use crate::domain::category::Category;
use crate::domain::compte::ProprietaireId;
use crate::domain::depense::{LigneDepenseCategorie, Mois, RepartitionDepenses};
use crate::domain::ports::lecture::{DepensesReadRepository, LectureError};
use crate::repository::bank_transactions::{FIELD_AMOUNT, TABLE};
use crate::repository::categories::{CategoryRow, into_category};
use crate::repository::chiffrement::dechiffrer_montant;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct SqlxDepensesRepository {
    db: Db,
    crypto: Arc<CryptoService>,
}

impl SqlxDepensesRepository {
    pub fn new(db: Db, crypto: Arc<CryptoService>) -> Self {
        Self { db, crypto }
    }

    async fn montants_depenses_par_categorie(
        &self,
        proprietaire: &ProprietaireId,
        mois: Mois,
    ) -> Result<HashMap<Option<Uuid>, i64>, LectureError> {
        let rows: Vec<(Option<Uuid>, Vec<u8>)> = sqlx::query_as(
            "SELECT t.category_id, t.amount_cents \
             FROM budgy.bank_transaction t \
             JOIN budgy.bank_account a ON a.id = t.bank_account_id \
             WHERE a.owner_id = $1 \
             AND COALESCE(t.booking_date, t.value_date) >= $2 \
             AND COALESCE(t.booking_date, t.value_date) < $3",
        )
        .bind(&proprietaire.0)
        .bind(mois.premier_jour())
        .bind(mois.premier_jour_mois_suivant())
        .fetch_all(&self.db)
        .await
        .map_err(|e| LectureError::Acces(e.to_string()))?;

        let mut totaux: HashMap<Option<Uuid>, i64> = HashMap::new();
        for (category_id, amount_blob) in rows {
            let montant = dechiffrer_montant(
                &self.crypto,
                &proprietaire.0,
                TABLE,
                FIELD_AMOUNT,
                &amount_blob,
            )
            .map_err(|e| LectureError::Acces(e.to_string()))?;
            if montant < 0 {
                *totaux.entry(category_id).or_insert(0) += montant.abs();
            }
        }
        Ok(totaux)
    }

    async fn categories_par_id(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<HashMap<Uuid, Category>, LectureError> {
        let rows = sqlx::query_as::<_, CategoryRow>(
            "SELECT id, owner_id, name, kind, color, icon, created_at \
             FROM budgy.category \
             WHERE owner_id IS NULL OR owner_id = $1",
        )
        .bind(&proprietaire.0)
        .fetch_all(&self.db)
        .await
        .map_err(|e| LectureError::Acces(e.to_string()))?;

        let mut categories = HashMap::with_capacity(rows.len());
        for row in rows {
            let category = into_category(row)?;
            categories.insert(category.id.0, category);
        }
        Ok(categories)
    }
}

impl DepensesReadRepository for SqlxDepensesRepository {
    async fn repartition_mensuelle_par_categorie(
        &self,
        proprietaire: &ProprietaireId,
        mois: Mois,
    ) -> Result<RepartitionDepenses, LectureError> {
        let totaux = self
            .montants_depenses_par_categorie(proprietaire, mois)
            .await?;
        if totaux.is_empty() {
            return Ok(RepartitionDepenses {
                total_cents: 0,
                lignes: Vec::new(),
            });
        }

        let categories = self.categories_par_id(proprietaire).await?;
        let total_cents: i64 = totaux.values().sum();

        let mut lignes: Vec<LigneDepenseCategorie> = totaux
            .into_iter()
            .map(|(category_id, montant_cents)| LigneDepenseCategorie {
                category: category_id.and_then(|id| categories.get(&id).cloned()),
                montant_cents,
            })
            .collect();

        trier_par_montant_decroissant(&mut lignes);

        Ok(RepartitionDepenses {
            total_cents,
            lignes,
        })
    }
}

fn trier_par_montant_decroissant(lignes: &mut [LigneDepenseCategorie]) {
    lignes.sort_by(|a, b| {
        b.montant_cents
            .cmp(&a.montant_cents)
            .then_with(|| ordre_categorie(&a.category).cmp(&ordre_categorie(&b.category)))
    });
}

fn ordre_categorie(category: &Option<Category>) -> (u8, &str) {
    match category {
        Some(c) => (0, c.name.as_str()),
        None => (1, ""),
    }
}
