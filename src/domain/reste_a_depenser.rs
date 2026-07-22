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
    let depenses_par_categorie = indexer_depenses(depenses);
    let mut lignes: Vec<ResteCategorie> = budgets
        .into_iter()
        .map(|budget| ligne_pour_budget(budget, &depenses_par_categorie, categories))
        .collect();
    trier_par_reste_croissant(&mut lignes);
    ResteADepenser { lignes }
}

fn indexer_depenses(depenses: &RepartitionDepenses) -> HashMap<Uuid, i64> {
    depenses
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
