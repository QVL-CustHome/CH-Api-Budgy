use crate::domain::agregation::{indexer_par_categorie, medianes_par_categorie};
use crate::domain::budget::Budget;
use crate::domain::category::{Category, CategoryId};
use crate::domain::depense::RepartitionDepenses;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ResteCategorie {
    pub category_id: CategoryId,
    pub category: Option<Category>,
    pub montant_prevu_cents: i64,
    pub depense_cents: i64,
    pub reste_cents: i64,
    pub depassement_cents: i64,
    pub depasse: bool,
}

#[derive(Debug, Clone)]
pub struct ResteADepenser {
    pub lignes: Vec<ResteCategorie>,
}

pub fn calculer_reste_a_depenser(
    budgets: Vec<Budget>,
    depenses: &RepartitionDepenses,
    categories: &HashMap<Uuid, Category>,
) -> ResteADepenser {
    let depenses_par_categorie = indexer_par_categorie(depenses);
    let mut lignes: Vec<ResteCategorie> = budgets
        .into_iter()
        .map(|budget| ligne_pour_budget(budget, &depenses_par_categorie, categories))
        .collect();
    trier_par_reste_croissant(&mut lignes);
    ResteADepenser { lignes }
}

/// Reste à dépenser **prédit** : à défaut de budget défini, le budget prévu de
/// chaque catégorie est la **médiane** de ses dépenses sur les mois d'historique
/// fournis, et non le seul mois précédent. La médiane évite qu'une dépense
/// exceptionnelle (un remboursement ponctuel, un gros achat) ne gonfle
/// l'enveloppe du mois suivant comme si elle se reproduisait tous les mois.
pub fn calculer_reste_a_depenser_predit(
    historique: &[RepartitionDepenses],
    depenses_courant: &RepartitionDepenses,
    categories: &HashMap<Uuid, Category>,
) -> ResteADepenser {
    let reel = indexer_par_categorie(depenses_courant);

    let mut lignes: Vec<ResteCategorie> = medianes_par_categorie(historique)
        .into_iter()
        .map(|(category_id, montant_prevu_cents)| {
            let depense_cents = reel.get(&category_id).copied().unwrap_or(0);
            ResteCategorie {
                category: categories.get(&category_id).cloned(),
                category_id: CategoryId(category_id),
                montant_prevu_cents,
                depense_cents,
                reste_cents: montant_prevu_cents - depense_cents,
                depassement_cents: (depense_cents - montant_prevu_cents).max(0),
                depasse: depense_cents > montant_prevu_cents,
            }
        })
        .collect();
    trier_par_reste_croissant(&mut lignes);
    ResteADepenser { lignes }
}

fn ligne_pour_budget(
    budget: Budget,
    depenses_par_categorie: &HashMap<Uuid, i64>,
    categories: &HashMap<Uuid, Category>,
) -> ResteCategorie {
    let montant_prevu_cents = budget.montant_prevu_cents;
    let depense_cents = depenses_par_categorie
        .get(&budget.category_id.0)
        .copied()
        .unwrap_or(0);
    ResteCategorie {
        category: categories.get(&budget.category_id.0).cloned(),
        category_id: budget.category_id,
        montant_prevu_cents,
        depense_cents,
        reste_cents: montant_prevu_cents - depense_cents,
        depassement_cents: (depense_cents - montant_prevu_cents).max(0),
        depasse: depense_cents > montant_prevu_cents,
    }
}

fn trier_par_reste_croissant(lignes: &mut [ResteCategorie]) {
    lignes.sort_by(|a, b| {
        a.reste_cents
            .cmp(&b.reste_cents)
            .then_with(|| nom_categorie(a).cmp(nom_categorie(b)))
    });
}

fn nom_categorie(ligne: &ResteCategorie) -> &str {
    ligne
        .category
        .as_ref()
        .map(|category| category.name.as_str())
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::category::CategoryKind;
    use crate::domain::depense::LigneDepenseCategorie;
    use chrono::Utc;

    fn categorie(n: u128, nom: &str) -> Category {
        Category {
            id: CategoryId(Uuid::from_u128(n)),
            owner_id: None,
            name: nom.to_string(),
            kind: CategoryKind::Depense,
            color: "#546E7A".to_string(),
            icon: "tag".to_string(),
            created_at: Utc::now(),
        }
    }

    fn mois(depenses: &[(&Category, i64)]) -> RepartitionDepenses {
        RepartitionDepenses {
            total_cents: depenses.iter().map(|(_, montant)| montant).sum(),
            lignes: depenses
                .iter()
                .map(|(category, montant_cents)| LigneDepenseCategorie {
                    category: Some((*category).clone()),
                    montant_cents: *montant_cents,
                })
                .collect(),
        }
    }

    fn index(categories: &[&Category]) -> HashMap<Uuid, Category> {
        categories
            .iter()
            .map(|category| (category.id.0, (*category).clone()))
            .collect()
    }

    fn ligne<'a>(reste: &'a ResteADepenser, nom: &str) -> Option<&'a ResteCategorie> {
        reste
            .lignes
            .iter()
            .find(|ligne| nom_categorie(ligne) == nom)
    }

    #[test]
    fn une_depense_exceptionnelle_ne_devient_pas_une_enveloppe_mensuelle() {
        let ponctuel = categorie(1, "Autres dépenses");
        let historique = [
            mois(&[(&ponctuel, 37_200)]), // une seule fois
            mois(&[]),
            mois(&[]),
        ];

        let reste = calculer_reste_a_depenser_predit(&historique, &mois(&[]), &index(&[&ponctuel]));

        assert!(
            ligne(&reste, "Autres dépenses").is_none(),
            "la médiane de [37200, 0, 0] vaut 0 : pas d'enveloppe prévue"
        );
    }

    #[test]
    fn une_depense_recurrente_donne_une_enveloppe_egale_a_la_mediane() {
        let factures = categorie(2, "Factures");
        let historique = [
            mois(&[(&factures, 29_983)]),
            mois(&[(&factures, 20_000)]),
            mois(&[(&factures, 25_000)]),
        ];

        let reste = calculer_reste_a_depenser_predit(&historique, &mois(&[]), &index(&[&factures]));

        let ligne = ligne(&reste, "Factures").expect("catégorie prévue");
        assert_eq!(ligne.montant_prevu_cents, 25_000, "médiane des trois mois");
        assert_eq!(ligne.reste_cents, 25_000);
    }

    #[test]
    fn les_depenses_du_mois_courant_sont_soustraites_de_l_enveloppe() {
        let courses = categorie(3, "Courses");
        let historique = [
            mois(&[(&courses, 10_000)]),
            mois(&[(&courses, 10_000)]),
            mois(&[(&courses, 10_000)]),
        ];

        let reste = calculer_reste_a_depenser_predit(
            &historique,
            &mois(&[(&courses, 7_500)]),
            &index(&[&courses]),
        );

        let ligne = ligne(&reste, "Courses").expect("catégorie prévue");
        assert_eq!(ligne.depense_cents, 7_500);
        assert_eq!(ligne.reste_cents, 2_500);
        assert!(!ligne.depasse);
    }

    #[test]
    fn un_depassement_est_signale() {
        let loisirs = categorie(4, "Loisirs");
        let historique = [
            mois(&[(&loisirs, 5_000)]),
            mois(&[(&loisirs, 5_000)]),
            mois(&[(&loisirs, 5_000)]),
        ];

        let reste = calculer_reste_a_depenser_predit(
            &historique,
            &mois(&[(&loisirs, 8_000)]),
            &index(&[&loisirs]),
        );

        let ligne = ligne(&reste, "Loisirs").expect("catégorie prévue");
        assert!(ligne.depasse);
        assert_eq!(ligne.reste_cents, -3_000);
        assert_eq!(ligne.depassement_cents, 3_000);
    }
}
