//! Détection des virements internes entre comptes d'un même propriétaire.
//!
//! Un virement d'un compte à l'autre apparaît deux fois dans les données
//! bancaires : en débit côté émetteur, en crédit côté destinataire. Sans
//! appariement, il est compté à la fois comme une dépense et comme un revenu,
//! ce qui gonfle le reste à dépenser et fausse le prévisionnel. On apparie donc
//! les deux faces pour les exclure des calculs.
//!
//! L'appariement ne peut pas se contenter du montant et de la date. Deux
//! mouvements sans rapport tombent régulièrement sur la même somme le même
//! jour ; et quand plusieurs débits identiques peuvent correspondre à un même
//! crédit, choisir « le premier » revient à tirer au sort lequel des deux
//! disparaîtra des totaux. On classe donc les couples par ressemblance de
//! libellé et on retient les meilleurs.

use crate::domain::bank_account::BankAccountId;
use crate::domain::transaction_bancaire::TransactionBancaireId;
use chrono::NaiveDate;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Écart maximum, en jours, entre le débit et le crédit d'un même virement
/// (les deux banques ne datent pas toujours l'opération le même jour).
const TOLERANCE_JOURS: i64 = 4;

/// En dessous de cette ressemblance de libellé, un couple n'est retenu que s'il
/// ne peut pas être confondu avec un autre (voir [`appariement_sans_doute`]).
/// Deux faces d'un même virement partagent presque toujours un mot — le nom du
/// titulaire, ou la mention « VIR ».
const RESSEMBLANCE_MINIMALE: f64 = 0.25;

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
    /// Libellé nettoyé : c'est lui qui départage deux couples également
    /// plausibles sur le montant et la date.
    pub libelle: String,
    /// L'utilisateur a rangé ce mouvement à la main dans une vraie catégorie
    /// (ni « Virements internes », ni aucune). Indice qu'il y voit une opération
    /// réelle — pas un veto, car un virement interne peut très bien avoir été
    /// catégorisé par mégarde.
    pub range_a_la_main: bool,
}

/// Apparie chaque débit avec le crédit qui lui ressemble le plus, sur un
/// **autre** compte du propriétaire, à quelques jours près. Renvoie les
/// identifiants des deux faces de chaque virement détecté.
pub fn detecter_transferts_internes(
    mouvements: &[MouvementCandidat],
) -> Vec<TransactionBancaireId> {
    let debits: Vec<&MouvementCandidat> =
        mouvements.iter().filter(|m| m.amount_cents < 0).collect();
    let credits: Vec<&MouvementCandidat> =
        mouvements.iter().filter(|m| m.amount_cents > 0).collect();

    let mut couples: Vec<(f64, &MouvementCandidat, &MouvementCandidat)> = Vec::new();
    for debit in &debits {
        for credit in &credits {
            if sont_les_deux_faces(debit, credit) {
                couples.push((ressemblance(&debit.libelle, &credit.libelle), debit, credit));
            }
        }
    }

    // Combien de partenaires possibles de chaque côté : un couple qui n'a
    // qu'une seule lecture possible mérite plus de confiance qu'un couple pris
    // parmi plusieurs.
    let mut partenaires_du_debit: HashMap<Uuid, usize> = HashMap::new();
    let mut partenaires_du_credit: HashMap<Uuid, usize> = HashMap::new();
    for (_, debit, credit) in &couples {
        *partenaires_du_debit.entry(debit.id.0).or_default() += 1;
        *partenaires_du_credit.entry(credit.id.0).or_default() += 1;
    }

    // Du plus ressemblant au moins ressemblant ; à égalité, ordre déterministe
    // pour que le résultat ne dépende pas de l'ordre de lecture en base.
    couples.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then_with(|| a.1.date.cmp(&b.1.date))
            .then_with(|| a.1.id.0.cmp(&b.1.id.0))
            .then_with(|| a.2.id.0.cmp(&b.2.id.0))
    });

    let mut debits_pris: HashSet<Uuid> = HashSet::new();
    let mut credits_pris: HashSet<Uuid> = HashSet::new();
    let mut apparies = Vec::new();
    for (score, debit, credit) in couples {
        if debits_pris.contains(&debit.id.0) || credits_pris.contains(&credit.id.0) {
            continue;
        }
        if score < RESSEMBLANCE_MINIMALE
            && !appariement_sans_doute(debit, credit, &partenaires_du_debit, &partenaires_du_credit)
        {
            continue;
        }
        debits_pris.insert(debit.id.0);
        credits_pris.insert(credit.id.0);
        apparies.push(debit.id.clone());
        apparies.push(credit.id.clone());
    }

    apparies
}

/// Un couple dont les libellés ne se ressemblent pas n'est accepté que s'il ne
/// prête à aucune confusion : une seule lecture possible des deux côtés, et
/// aucune des deux faces rangée à la main dans une vraie catégorie.
///
/// C'est ce qui laisse passer une alimentation de compte aux libellés sans mot
/// commun, tout en refusant de marier deux opérations sans rapport qui tombent
/// sur la même somme.
fn appariement_sans_doute(
    debit: &MouvementCandidat,
    credit: &MouvementCandidat,
    partenaires_du_debit: &HashMap<Uuid, usize>,
    partenaires_du_credit: &HashMap<Uuid, usize>,
) -> bool {
    partenaires_du_debit.get(&debit.id.0).copied().unwrap_or(0) == 1
        && partenaires_du_credit
            .get(&credit.id.0)
            .copied()
            .unwrap_or(0)
            == 1
        && !debit.range_a_la_main
        && !credit.range_a_la_main
}

fn sont_les_deux_faces(debit: &MouvementCandidat, credit: &MouvementCandidat) -> bool {
    credit.compte != debit.compte
        && credit.amount_cents == -debit.amount_cents
        && (credit.date - debit.date).num_days().abs() <= TOLERANCE_JOURS
}

/// Part des mots communs aux deux libellés (indice de Jaccard). Les deux faces
/// d'un virement partagent le nom du titulaire, souvent la mention « VIR » ;
/// deux opérations sans rapport ne partagent rien.
fn ressemblance(a: &str, b: &str) -> f64 {
    let mots = |texte: &str| -> HashSet<String> {
        texte
            .split_whitespace()
            .map(|mot| mot.to_uppercase())
            .collect()
    };
    let (a, b) = (mots(a), mots(b));
    let union = a.union(&b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(&b).count() as f64 / union as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compte(n: u128) -> BankAccountId {
        BankAccountId(Uuid::from_u128(n))
    }

    fn mouvement(id: u128, compte_n: u128, amount_cents: i64, jour: u32) -> MouvementCandidat {
        libelle(id, compte_n, amount_cents, jour, "VIR INST QUEVAL MARTIN")
    }

    fn libelle(
        id: u128,
        compte_n: u128,
        amount_cents: i64,
        jour: u32,
        libelle: &str,
    ) -> MouvementCandidat {
        MouvementCandidat {
            id: TransactionBancaireId(Uuid::from_u128(id)),
            compte: compte(compte_n),
            amount_cents,
            date: NaiveDate::from_ymd_opt(2026, 7, jour).expect("date valide"),
            libelle: libelle.to_string(),
            range_a_la_main: false,
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
        let mouvements = vec![
            mouvement(1, 10, -30_000, 6),
            mouvement(2, 10, -30_000, 7),
            mouvement(3, 20, 30_000, 6),
        ];

        let apparies = detecter_transferts_internes(&mouvements);
        assert_eq!(apparies.len(), 2, "une seule paire attendue");
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
            libelle(1, 10, -8_580, 10, "CARREFOUR ROUEN"),
            libelle(2, 10, 120_926, 30, "VIREMENT SALAIRE"),
        ];

        assert!(detecter_transferts_internes(&mouvements).is_empty());
    }

    /// Le cas réel du 31 juillet : deux débits de 200 € le même jour, un seul
    /// crédit en face. L'ancien code prenait le premier par UUID et sortait
    /// l'épargne des totaux ; c'est le libellé qui désigne la bonne face.
    #[test]
    fn entre_deux_debits_identiques_retient_celui_dont_le_libelle_correspond() {
        let epargne = libelle(1, 10, -20_000, 31, "WEB QUEVAL MARTIN");
        let virement = libelle(2, 10, -20_000, 31, "VIR INST VERS MARTIN QUEVAL");
        let credit = libelle(3, 20, 20_000, 31, "VIR INST QUEVAL MARTIN");

        let apparies = detecter_transferts_internes(&[epargne, virement, credit]);

        assert_eq!(
            ids(&apparies),
            vec![2, 3],
            "le virement doit être apparié, l'épargne rester comptée"
        );
    }

    /// Le faux positif réel du 5 août : un virement de 10 € à un tiers et une
    /// prime du même montant le lendemain. Rien de commun dans les libellés, et
    /// les deux ont été rangés à la main : on n'y touche pas.
    #[test]
    fn n_apparie_pas_deux_operations_sans_rapport_rangees_a_la_main() {
        let mut sortie = libelle(1, 10, -1_000, 5, "VIR INST VERS LOIC");
        sortie.range_a_la_main = true;
        let mut prime = libelle(2, 20, 1_000, 6, "PRIME OPERATIONS MISSION");
        prime.range_a_la_main = true;

        assert!(detecter_transferts_internes(&[sortie, prime]).is_empty());
    }

    /// L'alimentation de compte réelle du 6 juillet : aucun mot commun, mais un
    /// seul appariement possible et rien de catégorisé. On la reconnaît quand
    /// même — sans quoi 300 € apparaîtraient en dépense **et** en revenu.
    #[test]
    fn apparie_une_alimentation_de_compte_sans_libelle_commun_si_elle_est_sans_ambiguite() {
        let debit = libelle(1, 10, -30_000, 6, "BOURSORAMA BOULOGNE");
        let credit = libelle(2, 20, 30_000, 3, "ALIMENTATION CB");

        assert_eq!(
            ids(&detecter_transferts_internes(&[debit, credit])),
            vec![1, 2]
        );
    }

    /// Même cas, mais l'utilisateur a rangé le débit dans une vraie catégorie :
    /// le doute lui profite, le mouvement reste compté.
    #[test]
    fn ne_force_pas_un_appariement_douteux_sur_un_mouvement_categorise() {
        let mut debit = libelle(1, 10, -30_000, 6, "BOURSORAMA BOULOGNE");
        debit.range_a_la_main = true;
        let credit = libelle(2, 20, 30_000, 3, "ALIMENTATION CB");

        assert!(detecter_transferts_internes(&[debit, credit]).is_empty());
    }

    /// Deux couples possibles aux libellés muets : aucune lecture ne s'impose,
    /// on s'abstient plutôt que de tirer au sort.
    #[test]
    fn s_abstient_quand_plusieurs_lectures_sont_possibles_et_les_libelles_muets() {
        let mouvements = vec![
            libelle(1, 10, -5_000, 6, "PAIEMENT A"),
            libelle(2, 10, -5_000, 6, "PAIEMENT B"),
            libelle(3, 20, 5_000, 6, "RECETTE C"),
        ];

        assert!(detecter_transferts_internes(&mouvements).is_empty());
    }

    #[test]
    fn le_resultat_ne_depend_pas_de_l_ordre_de_lecture() {
        let a = libelle(1, 10, -20_000, 31, "WEB QUEVAL MARTIN");
        let b = libelle(2, 10, -20_000, 31, "VIR INST VERS MARTIN QUEVAL");
        let c = libelle(3, 20, 20_000, 31, "VIR INST QUEVAL MARTIN");

        let ordre_1 = ids(&detecter_transferts_internes(&[
            a.clone(),
            b.clone(),
            c.clone(),
        ]));
        let ordre_2 = ids(&detecter_transferts_internes(&[c, b, a]));

        assert_eq!(ordre_1, ordre_2);
    }
}
