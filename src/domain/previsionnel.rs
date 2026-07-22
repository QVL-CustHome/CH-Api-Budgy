use crate::domain::budget::Budget;
use crate::domain::category::{Category, CategoryId, CategoryKind};
use crate::domain::recurrence::normaliser_marchand;
use chrono::NaiveDate;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct OccurrenceRecurrente {
    pub category_id: Option<CategoryId>,
    pub label: String,
    pub amount_cents: i64,
    pub date: NaiveDate,
}

#[derive(Debug, Clone)]
pub struct LignePrevisionCategorie {
    pub category_id: Option<CategoryId>,
    pub category: Option<Category>,
    pub revenus_recurrents_cents: i64,
    pub depenses_recurrentes_cents: i64,
    pub budget_cents: i64,
}

#[derive(Debug, Clone)]
pub struct Previsionnel {
    pub solde_previsionnel_cents: i64,
    pub revenus_recurrents_cents: i64,
    pub depenses_recurrentes_cents: i64,
    pub budgets_cents: i64,
    pub lignes: Vec<LignePrevisionCategorie>,
    pub donnees_suffisantes: bool,
}

#[derive(Debug, Default, Clone, Copy)]
struct AgregatCategorie {
    revenus_recurrents_cents: i64,
    depenses_recurrentes_cents: i64,
    budget_cents: i64,
}

pub fn calculer_previsionnel(
    recurrents: Vec<OccurrenceRecurrente>,
    budgets: Vec<Budget>,
    categories: &HashMap<Uuid, Category>,
) -> Previsionnel {
    let donnees_suffisantes = !recurrents.is_empty();
    let previsions_mensuelles = derniere_occurrence_par_marchand(recurrents);

    let mut agregats: HashMap<Option<Uuid>, AgregatCategorie> = HashMap::new();
    for prevision in previsions_mensuelles {
        let cle = prevision.category_id.as_ref().map(|id| id.0);
        let kind = cle
            .and_then(|id| categories.get(&id))
            .map(|category| category.kind);
        let montant = prevision.amount_cents.abs();
        let entree = agregats.entry(cle).or_default();
        if est_depense(kind, prevision.amount_cents) {
            entree.depenses_recurrentes_cents += montant;
        } else {
            entree.revenus_recurrents_cents += montant;
        }
    }
    for budget in budgets {
        agregats
            .entry(Some(budget.category_id.0))
            .or_default()
            .budget_cents += budget.montant_prevu_cents;
    }

    let mut lignes: Vec<LignePrevisionCategorie> = agregats
        .into_iter()
        .map(|(cle, agregat)| LignePrevisionCategorie {
            category: cle.and_then(|id| categories.get(&id).cloned()),
            category_id: cle.map(CategoryId),
            revenus_recurrents_cents: agregat.revenus_recurrents_cents,
            depenses_recurrentes_cents: agregat.depenses_recurrentes_cents,
            budget_cents: agregat.budget_cents,
        })
        .collect();
    trier_lignes(&mut lignes);

    let revenus_recurrents_cents: i64 = lignes.iter().map(|l| l.revenus_recurrents_cents).sum();
    let depenses_recurrentes_cents: i64 = lignes.iter().map(|l| l.depenses_recurrentes_cents).sum();
    let budgets_cents: i64 = lignes.iter().map(|l| l.budget_cents).sum();

    Previsionnel {
        solde_previsionnel_cents: revenus_recurrents_cents
            - depenses_recurrentes_cents
            - budgets_cents,
        revenus_recurrents_cents,
        depenses_recurrentes_cents,
        budgets_cents,
        lignes,
        donnees_suffisantes,
    }
}

fn derniere_occurrence_par_marchand(
    recurrents: Vec<OccurrenceRecurrente>,
) -> Vec<OccurrenceRecurrente> {
    let mut derniere: HashMap<String, OccurrenceRecurrente> = HashMap::new();
    for occurrence in recurrents {
        let marchand = normaliser_marchand(&occurrence.label);
        let remplacer = match derniere.get(&marchand) {
            Some(reference) => occurrence_plus_recente(&occurrence, reference),
            None => true,
        };
        if remplacer {
            derniere.insert(marchand, occurrence);
        }
    }
    derniere.into_values().collect()
}

fn est_depense(kind: Option<CategoryKind>, amount_cents: i64) -> bool {
    match kind {
        Some(CategoryKind::Depense) => true,
        Some(CategoryKind::Revenu) => false,
        None => amount_cents < 0,
    }
}

fn occurrence_plus_recente(
    candidate: &OccurrenceRecurrente,
    reference: &OccurrenceRecurrente,
) -> bool {
    candidate.date > reference.date
        || (candidate.date == reference.date && candidate.amount_cents > reference.amount_cents)
}

fn trier_lignes(lignes: &mut [LignePrevisionCategorie]) {
    lignes.sort_by(|a, b| {
        rang_categorie(a)
            .cmp(&rang_categorie(b))
            .then_with(|| nom_categorie(a).cmp(nom_categorie(b)))
            .then_with(|| id_categorie(a).cmp(&id_categorie(b)))
    });
}

fn rang_categorie(ligne: &LignePrevisionCategorie) -> u8 {
    if ligne.category_id.is_some() { 0 } else { 1 }
}

fn nom_categorie(ligne: &LignePrevisionCategorie) -> &str {
    ligne
        .category
        .as_ref()
        .map(|category| category.name.as_str())
        .unwrap_or("")
}

fn id_categorie(ligne: &LignePrevisionCategorie) -> Uuid {
    ligne
        .category_id
        .as_ref()
        .map(|id| id.0)
        .unwrap_or_else(Uuid::nil)
}
