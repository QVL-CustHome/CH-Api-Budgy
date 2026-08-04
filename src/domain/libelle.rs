//! Nettoyage des libellés bancaires.
//!
//! Extrait le « tiers » (marchand / contrepartie) d'un libellé bancaire en retirant
//! les préfixes d'opération (VIREMENT EN VOTRE FAVEUR, PAIEMENT PAR CARTE...), les
//! civilités, les dates, les mois et les références numériques.
//!
//! Utilisé à deux endroits :
//! - affichage des transactions (on ne montre que le tiers, pas le bruit bancaire) ;
//! - dérivation automatique du motif d'une règle de catégorisation (l'utilisateur
//!   n'a rien à saisir, l'app trouve le fragment stable elle-même).

/// Préfixes d'opération, du plus long au plus court (on retire le premier qui matche).
const PREFIXES: &[&str] = &[
    "VIREMENT INSTANTANE RECU DE",
    "VIREMENT SEPA RECU DE",
    "VIREMENT EN VOTRE FAVEUR DE",
    "VIREMENT EN VOTRE FAVEUR",
    "VIREMENT INSTANTANE RECU",
    "VIREMENT INSTANTANE",
    "VIREMENT SEPA EMIS",
    "VIREMENT SEPA RECU",
    "VIREMENT SEPA",
    "VIREMENT RECU DE",
    "VIREMENT RECU",
    "VIREMENT EMIS",
    "VIREMENT",
    "PAIEMENT PAR CARTE",
    "PAIEMENT CARTE",
    "PAIEMENT",
    "PRELEVEMENT SEPA DE",
    "PRELEVEMENT SEPA",
    "PRELEVEMENT",
    "PRLV SEPA",
    "PRLV",
    "ACHAT CB",
    "ACHAT",
    "RETRAIT CB",
    "RETRAIT DAB",
    "RETRAIT",
    "REMISE CHEQUE",
    "REMISE",
    "COTISATION",
    "FRAIS",
    "CB",
];

/// Civilités retirées où qu'elles apparaissent dans le libellé.
const CIVILITES: &[&str] = &[
    "M.OU MME",
    "M. OU MME",
    "MR OU MME",
    "MME OU MR",
    "MLLE",
    "MME",
    "MR",
    "M.",
];

const MOIS: &[&str] = &[
    "JANVIER",
    "FEVRIER",
    "MARS",
    "AVRIL",
    "MAI",
    "JUIN",
    "JUILLET",
    "AOUT",
    "SEPTEMBRE",
    "OCTOBRE",
    "NOVEMBRE",
    "DECEMBRE",
];

/// Nombre maximum de mots conservés pour le tiers (motif court et général).
const MAX_TOKENS: usize = 5;

/// Extrait le tiers d'un libellé bancaire (marchand / contrepartie), en MAJUSCULES.
/// Ne renvoie jamais de chaîne vide : à défaut, retombe sur les premiers mots du libellé.
pub fn extraire_tiers(label: &str) -> String {
    let normalise = normaliser(label);
    let sans_prefixe = retirer_prefixe(&normalise);
    let sans_civilite = retirer_civilites(sans_prefixe);
    let tiers = filtrer_tokens(&sans_civilite);
    if tiers.trim().is_empty() {
        premiers_mots(&normalise, MAX_TOKENS)
    } else {
        tiers
    }
}

fn normaliser(label: &str) -> String {
    label
        .to_uppercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn retirer_prefixe(s: &str) -> &str {
    for prefixe in PREFIXES {
        if let Some(reste) = s.strip_prefix(prefixe) {
            if reste.is_empty() || reste.starts_with(' ') {
                let reste = reste.trim_start();
                // enlève un « DE » résiduel (ex. « VIREMENT RECU » puis « DE X »)
                return reste.strip_prefix("DE ").unwrap_or(reste).trim_start();
            }
        }
    }
    s
}

fn retirer_civilites(s: &str) -> String {
    let mut out = format!(" {s} ");
    for civilite in CIVILITES {
        let motif = format!(" {civilite} ");
        while let Some(pos) = out.find(&motif) {
            out.replace_range(pos..pos + motif.len(), " ");
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn est_token_volatile(token: &str) -> bool {
    // dates, références, montants, numéros de carte
    if token.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    // masque de carte (XXXX, ****)
    if !token.is_empty() && token.chars().all(|c| c == 'X' || c == '*') {
        return true;
    }
    MOIS.contains(&token)
}

fn filtrer_tokens(s: &str) -> String {
    s.split_whitespace()
        .filter(|token| !est_token_volatile(token))
        .take(MAX_TOKENS)
        .collect::<Vec<_>>()
        .join(" ")
}

fn premiers_mots(s: &str, n: usize) -> String {
    s.split_whitespace().take(n).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn salaire_bluesoft_devient_bluesoft() {
        let brut = "VIREMENT EN VOTRE FAVEUR BLUE SOFT VIREMENT-SALAIRE-JUIN-26-29047 VIREMENT-SALAIRE-JUIN-26-29047";
        assert_eq!(extraire_tiers(brut), "BLUE SOFT");
    }

    #[test]
    fn virement_particulier_garde_le_nom() {
        let brut = "VIREMENT EN VOTRE FAVEUR DE M.OU MME QUEVAL CEDRIC";
        assert_eq!(extraire_tiers(brut), "QUEVAL CEDRIC");
    }

    #[test]
    fn paiement_carte_garde_le_marchand() {
        let brut = "PAIEMENT PAR CARTE 1234 CARREFOUR MARKET 05/08/26";
        assert_eq!(extraire_tiers(brut), "CARREFOUR MARKET");
    }

    #[test]
    fn prelevement_garde_le_creancier() {
        let brut = "PRELEVEMENT SEPA EDF 08/2026 REF 998877";
        assert_eq!(extraire_tiers(brut), "EDF");
    }

    #[test]
    fn insensible_a_la_casse() {
        assert_eq!(extraire_tiers("paiement par carte carrefour"), "CARREFOUR");
    }

    #[test]
    fn tiers_vide_apres_filtrage_retombe_sur_le_libelle() {
        // que du bruit après préfixe -> fallback sur les premiers mots normalisés
        assert_eq!(extraire_tiers("VIREMENT 123 456"), "VIREMENT 123 456");
    }
}
