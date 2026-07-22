use crate::domain::transaction_bancaire::TransactionBancaireId;
use chrono::NaiveDate;
use std::collections::HashMap;

const OCCURRENCES_MINIMALES: usize = 3;
const INTERVALLE_MENSUEL_MIN_JOURS: i64 = 26;
const INTERVALLE_MENSUEL_MAX_JOURS: i64 = 35;
const TOLERANCE_MONTANT_CENTS: i64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecurrenceInterval {
    Mensuel,
}

impl RecurrenceInterval {
    pub fn as_str(&self) -> &'static str {
        match self {
            RecurrenceInterval::Mensuel => "monthly",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "monthly" => Some(RecurrenceInterval::Mensuel),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OccurrenceTransaction {
    pub id: TransactionBancaireId,
    pub label: String,
    pub amount_cents: i64,
    pub date: NaiveDate,
}

#[derive(Debug, Clone)]
pub struct TransactionRecurrente {
    pub id: TransactionBancaireId,
    pub interval: RecurrenceInterval,
}

pub fn detecter_recurrences(occurrences: &[OccurrenceTransaction]) -> Vec<TransactionRecurrente> {
    let mut par_marchand: HashMap<String, Vec<&OccurrenceTransaction>> = HashMap::new();
    for occurrence in occurrences {
        par_marchand
            .entry(normaliser_marchand(&occurrence.label))
            .or_default()
            .push(occurrence);
    }

    let mut recurrentes = Vec::new();
    for groupe in par_marchand.values() {
        for grappe in regrouper_par_montant(groupe) {
            if grappe_est_mensuelle(&grappe) {
                for occurrence in grappe {
                    recurrentes.push(TransactionRecurrente {
                        id: occurrence.id.clone(),
                        interval: RecurrenceInterval::Mensuel,
                    });
                }
            }
        }
    }
    recurrentes
}

pub fn normaliser_marchand(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn regrouper_par_montant<'a>(
    occurrences: &[&'a OccurrenceTransaction],
) -> Vec<Vec<&'a OccurrenceTransaction>> {
    let mut triees = occurrences.to_vec();
    triees.sort_by_key(|occurrence| occurrence.amount_cents);

    let mut grappes: Vec<Vec<&OccurrenceTransaction>> = Vec::new();
    for occurrence in triees {
        match grappes.last_mut() {
            Some(grappe) if montants_proches(grappe[0].amount_cents, occurrence.amount_cents) => {
                grappe.push(occurrence);
            }
            _ => grappes.push(vec![occurrence]),
        }
    }
    grappes
}

fn montants_proches(a: i64, b: i64) -> bool {
    (a - b).abs() <= TOLERANCE_MONTANT_CENTS
}

fn grappe_est_mensuelle(grappe: &[&OccurrenceTransaction]) -> bool {
    if grappe.len() < OCCURRENCES_MINIMALES {
        return false;
    }

    let mut dates: Vec<NaiveDate> = grappe.iter().map(|occurrence| occurrence.date).collect();
    dates.sort();

    let intervalles_mensuels = dates
        .windows(2)
        .filter(|paire| {
            let jours = (paire[1] - paire[0]).num_days();
            (INTERVALLE_MENSUEL_MIN_JOURS..=INTERVALLE_MENSUEL_MAX_JOURS).contains(&jours)
        })
        .count();

    intervalles_mensuels >= OCCURRENCES_MINIMALES - 1
}
