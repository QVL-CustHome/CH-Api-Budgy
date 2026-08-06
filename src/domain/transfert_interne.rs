//! Détection des virements internes entre comptes d'un même propriétaire.
//!
//! Un virement d'un compte à l'autre apparaît deux fois dans les données
//! bancaires : en débit côté émetteur, en crédit côté destinataire. Sans
//! appariement, il est compté à la fois comme une dépense et comme un revenu,
//! ce qui gonfle le reste à dépenser et fausse le prévisionnel. On apparie donc
//! les deux faces pour les exclure des calculs.

use crate::domain::bank_account::BankAccountId;
use crate::domain::transaction_bancaire::TransactionBancaireId;
use chrono::NaiveDate;
use uuid::Uuid;

/// Écart maximum, en jours, entre le débit et le crédit d'un même virement
/// (les deux banques ne datent pas toujours l'opération le même jour).
const TOLERANCE_JOURS: i64 = 4;

/// Catégorie système « Virements internes » (UUID figé, semé en migration).
/// Y ranger une transaction l'exclut des dépenses et des revenus : c'est
/// l'échappatoire manuelle quand le compte en face n'est pas rattaché à Budgy.
pub const CATEGORIE_VIREMENTS_INTERNES: Uuid =
    Uuid::from_u128(0x00000000_0000_4000_8000_000000000001);

#[derive(Debug, Clone)]
pub struct MouvementCandidat {
    pub id: TransactionBancaireId,
    pub compte: BankAccountId,
    pub amount_cents: i64,
    pub date: NaiveDate,
}

/// Apparie chaque débit avec un crédit de même montant situé sur un **autre**
/// compte du propriétaire, à quelques jours près. Renvoie les identifiants des
/// deux faces de chaque virement détecté. Un mouvement n'est apparié qu'une
/// fois : deux virements identiques le même mois donnent deux paires distinctes.
pub fn detecter_transferts_internes(
    mouvements: &[MouvementCandidat],
) -> Vec<TransactionBancaireId> {
    let debits = trier(mouvements.iter().filter(|m| m.amount_cents < 0).collect());
    let credits = trier(mouvements.iter().filter(|m| m.amount_cents > 0).collect());

    let mut credit_consomme = vec![false; credits.len()];
    let mut apparies = Vec::new();

    for debit in debits {
        let correspondance = credits.iter().enumerate().position(|(index, credit)| {
            !credit_consomme[index] && sont_les_deux_faces(debit, credit)
        });
        if let Some(index) = correspondance {
            credit_consomme[index] = true;
            apparies.push(debit.id.clone());
            apparies.push(credits[index].id.clone());
        }
    }

    apparies
}

/// Tri déterministe (date puis identifiant) pour que l'appariement ne dépende
/// pas de l'ordre de lecture en base.
fn trier(mut mouvements: Vec<&MouvementCandidat>) -> Vec<&MouvementCandidat> {
    mouvements.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.id.0.cmp(&b.id.0)));
    mouvements
}

fn sont_les_deux_faces(debit: &MouvementCandidat, credit: &MouvementCandidat) -> bool {
    credit.compte != debit.compte
        && credit.amount_cents == -debit.amount_cents
        && (credit.date - debit.date).num_days().abs() <= TOLERANCE_JOURS
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn compte(n: u128) -> BankAccountId {
        BankAccountId(Uuid::from_u128(n))
    }

    fn mouvement(id: u128, compte_n: u128, amount_cents: i64, jour: u32) -> MouvementCandidat {
        MouvementCandidat {
            id: TransactionBancaireId(Uuid::from_u128(id)),
            compte: compte(compte_n),
            amount_cents,
            date: NaiveDate::from_ymd_opt(2026, 7, jour).expect("date valide"),
        }
    }

    fn ids(apparies: &[TransactionBancaireId]) -> Vec<u128> {
        let mut sortie: Vec<u128> = apparies.iter().map(|id| id.0.as_u128()).collect();
        sortie.sort_unstable();
        sortie
    }

    #[test]
    fn apparie_un_debit_et_un_credit_de_meme_montant_sur_deux_comptes() {
        let mouvements = vec![mouvement(1, 10, -30_000, 6), mouvement(2, 20, 30_000, 6)];

        assert_eq!(ids(&detecter_transferts_internes(&mouvements)), vec![1, 2]);
    }

    #[test]
    fn tolere_un_decalage_de_quelques_jours_entre_les_deux_faces() {
        let mouvements = vec![mouvement(1, 10, -30_000, 6), mouvement(2, 20, 30_000, 3)];

        assert_eq!(ids(&detecter_transferts_internes(&mouvements)), vec![1, 2]);
    }

    #[test]
    fn n_apparie_pas_au_dela_de_la_tolerance() {
        let mouvements = vec![mouvement(1, 10, -30_000, 1), mouvement(2, 20, 30_000, 20)];

        assert!(detecter_transferts_internes(&mouvements).is_empty());
    }

    #[test]
    fn n_apparie_pas_deux_mouvements_du_meme_compte() {
        // Un remboursement sur le même compte n'est pas un virement interne.
        let mouvements = vec![mouvement(1, 10, -30_000, 6), mouvement(2, 10, 30_000, 6)];

        assert!(detecter_transferts_internes(&mouvements).is_empty());
    }

    #[test]
    fn n_apparie_pas_des_montants_differents() {
        let mouvements = vec![mouvement(1, 10, -30_000, 6), mouvement(2, 20, 29_000, 6)];

        assert!(detecter_transferts_internes(&mouvements).is_empty());
    }

    #[test]
    fn un_credit_ne_sert_qu_a_un_seul_debit() {
        // Deux débits identiques mais un seul crédit en face : une seule paire.
        let mouvements = vec![
            mouvement(1, 10, -30_000, 6),
            mouvement(2, 10, -30_000, 7),
            mouvement(3, 20, 30_000, 6),
        ];

        let apparies = detecter_transferts_internes(&mouvements);
        assert_eq!(apparies.len(), 2, "une seule paire attendue");
        assert_eq!(ids(&apparies), vec![1, 3]);
    }

    #[test]
    fn apparie_deux_virements_distincts_du_meme_montant() {
        let mouvements = vec![
            mouvement(1, 10, -20_000, 6),
            mouvement(2, 10, -20_000, 20),
            mouvement(3, 20, 20_000, 6),
            mouvement(4, 20, 20_000, 20),
        ];

        assert_eq!(
            ids(&detecter_transferts_internes(&mouvements)),
            vec![1, 2, 3, 4]
        );
    }

    #[test]
    fn laisse_les_vraies_depenses_et_revenus_intacts() {
        let mouvements = vec![
            mouvement(1, 10, -8_580, 10),  // courses
            mouvement(2, 10, 120_926, 30), // salaire
        ];

        assert!(detecter_transferts_internes(&mouvements).is_empty());
    }
}
