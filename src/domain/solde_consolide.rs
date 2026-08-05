use crate::domain::ports::lecture::CompteAvecSolde;

#[derive(Debug, Clone)]
pub struct SoldeConsolide {
    pub total_cents: i64,
    /// Total à venir : présent uniquement si au moins un compte expose un solde
    /// à venir. Par compte, on retient l'`expected` s'il existe, sinon le solde
    /// courant (pour que le total reste cohérent même quand une seule banque le
    /// fournit).
    pub total_a_venir_cents: Option<i64>,
    pub comptes: Vec<CompteAvecSolde>,
}

impl SoldeConsolide {
    pub fn consolider(comptes: Vec<CompteAvecSolde>) -> Self {
        let total_cents = comptes
            .iter()
            .filter_map(|compte| compte.solde.as_ref())
            .fold(0i64, |acc, solde| acc.saturating_add(solde.amount_cents));

        let total_a_venir_cents = if comptes.iter().any(|c| c.solde_a_venir.is_some()) {
            Some(comptes.iter().fold(0i64, |acc, compte| {
                let montant = compte
                    .solde_a_venir
                    .as_ref()
                    .or(compte.solde.as_ref())
                    .map(|solde| solde.amount_cents)
                    .unwrap_or(0);
                acc.saturating_add(montant)
            }))
        } else {
            None
        };

        Self {
            total_cents,
            total_a_venir_cents,
            comptes,
        }
    }
}
