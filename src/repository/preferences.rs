use crate::db::Db;
use crate::domain::compte::ProprietaireId;
use crate::domain::cycle::JourDebutMois;
use crate::domain::ports::ecriture::EcritureError;
use crate::domain::ports::lecture::LectureError;

#[derive(Clone)]
pub struct SqlxPreferencesRepository {
    db: Db,
}

impl SqlxPreferencesRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Jour de départ du mois budgétaire. Un utilisateur qui n'a rien réglé
    /// n'a pas de ligne : on retombe sur le 1er, soit le mois calendaire.
    pub async fn jour_debut_mois(
        &self,
        proprietaire: &ProprietaireId,
    ) -> Result<JourDebutMois, LectureError> {
        let ligne: Option<(i16,)> = sqlx::query_as(
            "SELECT jour_debut_mois FROM budgy.preferences_utilisateur WHERE owner_id = $1",
        )
        .bind(&proprietaire.0)
        .fetch_optional(&self.db)
        .await
        .map_err(|e| LectureError::Acces(e.to_string()))?;

        let Some((jour,)) = ligne else {
            return Ok(JourDebutMois::PREMIER);
        };
        // La contrainte CHECK borne déjà la colonne ; en cas de valeur aberrante
        // on préfère le comportement par défaut à une erreur de lecture.
        Ok(JourDebutMois::nouveau(jour as u32).unwrap_or(JourDebutMois::PREMIER))
    }

    pub async fn definir_jour_debut_mois(
        &self,
        proprietaire: &ProprietaireId,
        jour: JourDebutMois,
    ) -> Result<JourDebutMois, EcritureError> {
        sqlx::query(
            "INSERT INTO budgy.preferences_utilisateur (owner_id, jour_debut_mois) \
             VALUES ($1, $2) \
             ON CONFLICT (owner_id) DO UPDATE \
             SET jour_debut_mois = EXCLUDED.jour_debut_mois, updated_at = now()",
        )
        .bind(&proprietaire.0)
        .bind(jour.valeur() as i16)
        .execute(&self.db)
        .await
        .map_err(|e| EcritureError::Acces(e.to_string()))?;
        Ok(jour)
    }
}
