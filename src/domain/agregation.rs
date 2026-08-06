//! Agrégations statistiques sur un historique mensuel.
//!
//! La médiane sert de base aux prédictions (reste à dépenser, revenus
//! prévisionnels) : contrairement à la moyenne ou au simple mois précédent,
//! elle ignore les montants exceptionnels d'un mois isolé.

use crate::domain::depense::RepartitionDepenses;
use std::collections::HashMap;
use uuid::Uuid;

/// Nombre de mois passés servant de base aux prédictions (médiane glissante).
pub const MOIS_HISTORIQUE: usize = 3;

/// Montants d'un mois indexés par catégorie. Les lignes sans catégorie sont
/// ignorées : on ne prédit que ce qu'on sait attribuer.
pub fn indexer_par_categorie(repartition: &RepartitionDepenses) -> HashMap<Uuid, i64> {
    repartition
        .lignes
        .iter()
        .filter_map(|ligne| {
            ligne
                .category
                .as_ref()
                .map(|category| (category.id.0, ligne.montant_cents))
        })
        .collect()
}

/// Médiane, catégorie par catégorie, des montants mensuels de l'historique.
/// Un mois où la catégorie n'apparaît pas compte pour zéro : une dépense (ou une
/// recette) unique ne doit pas passer pour la norme. Seules les médianes
/// strictement positives sont renvoyées.
pub fn medianes_par_categorie(historique: &[RepartitionDepenses]) -> HashMap<Uuid, i64> {
    let mut montants_par_categorie: HashMap<Uuid, Vec<i64>> = HashMap::new();
    for mois in historique {
        for (category_id, montant_cents) in indexer_par_categorie(mois) {
            montants_par_categorie
                .entry(category_id)
                .or_default()
                .push(montant_cents);
        }
    }

    montants_par_categorie
        .into_iter()
        .filter_map(|(category_id, mut montants)| {
            montants.resize(historique.len(), 0);
            let mediane = mediane(&mut montants);
            (mediane > 0).then_some((category_id, mediane))
        })
        .collect()
}

/// Médiane d'une série de montants (moyenne des deux valeurs centrales si le
/// nombre de valeurs est pair). Série vide -> 0.
pub fn mediane(valeurs: &mut [i64]) -> i64 {
    if valeurs.is_empty() {
        return 0;
    }
    valeurs.sort_unstable();
    let milieu = valeurs.len() / 2;
    if valeurs.len().is_multiple_of(2) {
        (valeurs[milieu - 1] + valeurs[milieu]) / 2
    } else {
        valeurs[milieu]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::category::{Category, CategoryId, CategoryKind};
    use crate::domain::depense::LigneDepenseCategorie;
    use chrono::Utc;

    fn categorie(n: u128) -> Category {
        Category {
            id: CategoryId(Uuid::from_u128(n)),
            owner_id: None,
            name: format!("Catégorie {n}"),
            kind: CategoryKind::Depense,
            color: "#546E7A".to_string(),
            icon: "tag".to_string(),
            created_at: Utc::now(),
        }
    }

    fn mois(montants: &[(&Category, i64)]) -> RepartitionDepenses {
        RepartitionDepenses {
            total_cents: montants.iter().map(|(_, montant)| montant).sum(),
            lignes: montants
                .iter()
                .map(|(category, montant_cents)| LigneDepenseCategorie {
                    category: Some((*category).clone()),
                    montant_cents: *montant_cents,
                })
                .collect(),
        }
    }

    #[test]
    fn mediane_serie_impaire_paire_et_vide() {
        assert_eq!(mediane(&mut [10, 30, 20]), 20);
        assert_eq!(mediane(&mut [10, 20, 30, 40]), 25);
        assert_eq!(mediane(&mut []), 0);
    }

    #[test]
    fn un_montant_isole_ne_donne_pas_de_mediane() {
        let ponctuel = categorie(1);
        let historique = [mois(&[(&ponctuel, 37_200)]), mois(&[]), mois(&[])];

        assert!(
            medianes_par_categorie(&historique).is_empty(),
            "la médiane de [37200, 0, 0] vaut 0 : rien à prédire"
        );
    }

    #[test]
    fn un_montant_mensuel_donne_sa_mediane() {
        let recurrent = categorie(2);
        let historique = [
            mois(&[(&recurrent, 29_983)]),
            mois(&[(&recurrent, 20_000)]),
            mois(&[(&recurrent, 25_000)]),
        ];

        let medianes = medianes_par_categorie(&historique);

        assert_eq!(medianes.get(&recurrent.id.0), Some(&25_000));
    }

    #[test]
    fn une_categorie_presente_deux_mois_sur_trois_reste_predite() {
        let recurrent = categorie(3);
        let historique = [
            mois(&[(&recurrent, 10_000)]),
            mois(&[(&recurrent, 10_000)]),
            mois(&[]),
        ];

        // médiane de [0, 10000, 10000] = 10000
        assert_eq!(
            medianes_par_categorie(&historique).get(&recurrent.id.0),
            Some(&10_000)
        );
    }

    #[test]
    fn les_lignes_sans_categorie_sont_ignorees() {
        let repartition = RepartitionDepenses {
            total_cents: 5_000,
            lignes: vec![LigneDepenseCategorie {
                category: None,
                montant_cents: 5_000,
            }],
        };

        assert!(indexer_par_categorie(&repartition).is_empty());
    }
}
