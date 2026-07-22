use crate::domain::ports::lecture::CompteAvecSolde;

#[derive(Debug, Clone)]
pub struct SoldeConsolide {
    pub total_cents: i64,
    pub comptes: Vec<CompteAvecSolde>,
}

impl SoldeConsolide {
    pub fn consolider(comptes: Vec<CompteAvecSolde>) -> Self {
        let total_cents = comptes
            .iter()
            .filter_map(|compte| compte.solde.as_ref())
            .fold(0i64, |acc, solde| acc.saturating_add(solde.amount_cents));

        Self {
            total_cents,
            comptes,
        }
    }
}
