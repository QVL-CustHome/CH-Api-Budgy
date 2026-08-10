use crate::domain::agregation::indexer_par_categorie;
use crate::domain::category::{Category, CategoryId};
use crate::domain::depense::RepartitionDepenses;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Ce qu'une catégorie promet encore d'ici la fin du cycle.
#[derive(Debug, Clone)]
pub struct LignePrevisionCategorie {
    pub category_id: Option<CategoryId>,
    pub category: Option<Category>,
    /// Montant attendu sur le cycle entier (médiane des cycles passés).
    pub prevu_cents: i64,
    /// Déjà constaté depuis le début du cycle.
    pub realise_cents: i64,
    /// Ce qui reste à venir : `prevu - realise`, jamais négatif.
    pub restant_cents: i64,
}

#[derive(Debug, Clone)]
pub struct Previsionnel {
    /// Solde attendu au dernier jour du cycle.
    pub solde_previsionnel_cents: i64,
    pub solde_actuel_cents: i64,
    pub revenus_restants_cents: i64,
    pub depenses_restantes_cents: i64,
    pub lignes: Vec<LignePrevisionCategorie>,
    pub donnees_suffisantes: bool,
}

/// Prévisions d'un cycle, catégorie par catégorie, en centimes positifs.
pub type PrevisionsParCategorie = HashMap<Uuid, i64>;

/// Projette le solde à la fin du cycle budgétaire.
///
/// ```text
/// solde à la fin = solde actuel
///                + revenus encore attendus
///                - dépenses encore attendues
/// ```
///
/// Le calcul répond à une question de trésorerie — « combien me restera-t-il
/// avant la prochaine paie ? » — et non à une question de flux. La version
/// précédente additionnait revenus et dépenses récurrentes d'un mois entier
/// sans jamais regarder le solde : elle recomptait un salaire déjà encaissé,
/// ignorait ce qui avait déjà été dépensé depuis le début du cycle, et ne
/// voyait que les montants fixes répétés — donc ni les courses, ni les sorties,
/// qui pèsent le plus lourd.
///
/// Le reste à venir se lit catégorie par catégorie : `prévu - déjà réalisé`,
/// borné à zéro. Une catégorie déjà dépassée ne rend pas d'argent, elle ne
/// promet simplement plus rien.
pub fn calculer_previsionnel(
    solde_actuel_cents: i64,
    depenses_prevues: PrevisionsParCategorie,
    revenus_prevus: PrevisionsParCategorie,
    depenses_realisees: &RepartitionDepenses,
    revenus_realises: &RepartitionDepenses,
    categories: &HashMap<Uuid, Category>,
) -> Previsionnel {
    let donnees_suffisantes = !depenses_prevues.is_empty() || !revenus_prevus.is_empty();

    let depenses_reelles = indexer_par_categorie(depenses_realisees);
    let revenus_reels = indexer_par_categorie(revenus_realises);

    let mut lignes = Vec::new();
    let mut revenus_restants_cents = 0i64;
    let mut depenses_restantes_cents = 0i64;

    // Une catégorie prévue mais absente du cycle courant compte pour zéro de
    // réalisé : tout son montant reste à venir.
    for (category_id, prevu_cents) in &depenses_prevues {
        let realise_cents = depenses_reelles.get(category_id).copied().unwrap_or(0);
        let restant_cents = (prevu_cents - realise_cents).max(0);
        depenses_restantes_cents += restant_cents;
        lignes.push(LignePrevisionCategorie {
            category: categories.get(category_id).cloned(),
            category_id: Some(CategoryId(*category_id)),
            prevu_cents: *prevu_cents,
            realise_cents,
            restant_cents,
        });
    }

    for (category_id, prevu_cents) in &revenus_prevus {
        let realise_cents = revenus_reels.get(category_id).copied().unwrap_or(0);
        let restant_cents = (prevu_cents - realise_cents).max(0);
        revenus_restants_cents += restant_cents;
        lignes.push(LignePrevisionCategorie {
            category: categories.get(category_id).cloned(),
            category_id: Some(CategoryId(*category_id)),
            prevu_cents: *prevu_cents,
            realise_cents,
            restant_cents,
        });
    }

    trier_lignes(&mut lignes);

    Previsionnel {
        solde_previsionnel_cents: solde_actuel_cents + revenus_restants_cents
            - depenses_restantes_cents,
        solde_actuel_cents,
        revenus_restants_cents,
        depenses_restantes_cents,
        lignes,
        donnees_suffisantes,
    }
}

/// Ce qui reste à venir d'abord, puis par nom : l'utilisateur cherche ce qui
/// va encore le ponctionner.
fn trier_lignes(lignes: &mut [LignePrevisionCategorie]) {
    lignes.sort_by(|a, b| {
        b.restant_cents.cmp(&a.restant_cents).then_with(|| {
            let nom = |ligne: &LignePrevisionCategorie| {
                ligne
                    .category
                    .as_ref()
                    .map(|c| c.name.clone())
                    .unwrap_or_default()
            };
            nom(a).cmp(&nom(b))
        })
    });
}

/// Catégories de revenu, pour séparer les deux sens dans les prévisions.
pub fn categories_de_revenu(categories: &HashMap<Uuid, Category>) -> HashSet<Uuid> {
    use crate::domain::category::CategoryKind;
    categories
        .iter()
        .filter(|(_, category)| category.kind == CategoryKind::Revenu)
        .map(|(id, _)| *id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::category::CategoryKind;
    use crate::domain::depense::LigneDepenseCategorie;
    use chrono::Utc;

    fn categorie(nom: &str, kind: CategoryKind) -> Category {
        Category {
            id: CategoryId(Uuid::new_v4()),
            owner_id: None,
            name: nom.to_string(),
            kind,
            color: "#000000".to_string(),
            icon: "tag".to_string(),
            created_at: Utc::now(),
        }
    }

    fn repartition(lignes: Vec<(&Category, i64)>) -> RepartitionDepenses {
        RepartitionDepenses {
            total_cents: lignes.iter().map(|(_, montant)| montant).sum(),
            lignes: lignes
                .into_iter()
                .map(|(category, montant_cents)| LigneDepenseCategorie {
                    category: Some(category.clone()),
                    montant_cents,
                })
                .collect(),
        }
    }

    fn index(categories: &[&Category]) -> HashMap<Uuid, Category> {
        categories.iter().map(|c| (c.id.0, (*c).clone())).collect()
    }

    #[test]
    fn le_solde_projete_part_du_solde_actuel() {
        let courses = categorie("Courses", CategoryKind::Depense);
        let index = index(&[&courses]);

        let previsionnel = calculer_previsionnel(
            100_000,
            HashMap::from([(courses.id.0, 30_000)]),
            HashMap::new(),
            &repartition(vec![]),
            &repartition(vec![]),
            &index,
        );

        // 1 000 € en poche, 300 € de courses encore à venir.
        assert_eq!(previsionnel.solde_previsionnel_cents, 70_000);
        assert_eq!(previsionnel.solde_actuel_cents, 100_000);
    }

    #[test]
    fn un_salaire_deja_encaisse_n_est_pas_recompte() {
        // Le défaut d'origine : le salaire tombé en début de cycle était déjà
        // dans le solde, et le prévisionnel l'ajoutait une seconde fois.
        let salaire = categorie("Salaire", CategoryKind::Revenu);
        let index = index(&[&salaire]);

        let previsionnel = calculer_previsionnel(
            160_000,
            HashMap::new(),
            HashMap::from([(salaire.id.0, 161_390)]),
            &repartition(vec![]),
            &repartition(vec![(&salaire, 161_390)]),
            &index,
        );

        assert_eq!(previsionnel.revenus_restants_cents, 0);
        assert_eq!(previsionnel.solde_previsionnel_cents, 160_000);
    }

    #[test]
    fn les_depenses_deja_faites_sont_deduites_du_reste_a_venir() {
        let courses = categorie("Courses", CategoryKind::Depense);
        let index = index(&[&courses]);

        let previsionnel = calculer_previsionnel(
            100_000,
            HashMap::from([(courses.id.0, 40_000)]),
            HashMap::new(),
            &repartition(vec![(&courses, 25_000)]),
            &repartition(vec![]),
            &index,
        );

        // 400 € prévus, 250 € déjà passés : il n'en reste que 150 à venir.
        assert_eq!(previsionnel.depenses_restantes_cents, 15_000);
        assert_eq!(previsionnel.solde_previsionnel_cents, 85_000);
    }

    #[test]
    fn une_categorie_deja_depassee_ne_rend_pas_d_argent() {
        let loisirs = categorie("Loisirs", CategoryKind::Depense);
        let index = index(&[&loisirs]);

        let previsionnel = calculer_previsionnel(
            100_000,
            HashMap::from([(loisirs.id.0, 10_000)]),
            HashMap::new(),
            &repartition(vec![(&loisirs, 39_000)]),
            &repartition(vec![]),
            &index,
        );

        assert_eq!(previsionnel.depenses_restantes_cents, 0);
        assert_eq!(previsionnel.solde_previsionnel_cents, 100_000);
    }

    #[test]
    fn un_revenu_partiellement_recu_ne_promet_que_le_complement() {
        let salaire = categorie("Salaire", CategoryKind::Revenu);
        let index = index(&[&salaire]);

        let previsionnel = calculer_previsionnel(
            50_000,
            HashMap::new(),
            HashMap::from([(salaire.id.0, 160_000)]),
            &repartition(vec![]),
            &repartition(vec![(&salaire, 100_000)]),
            &index,
        );

        assert_eq!(previsionnel.revenus_restants_cents, 60_000);
        assert_eq!(previsionnel.solde_previsionnel_cents, 110_000);
    }

    #[test]
    fn sans_historique_les_donnees_sont_declarees_insuffisantes() {
        let previsionnel = calculer_previsionnel(
            100_000,
            HashMap::new(),
            HashMap::new(),
            &repartition(vec![]),
            &repartition(vec![]),
            &HashMap::new(),
        );

        assert!(!previsionnel.donnees_suffisantes);
        assert_eq!(previsionnel.solde_previsionnel_cents, 100_000);
    }

    #[test]
    fn les_lignes_montrent_d_abord_ce_qui_reste_a_venir() {
        let courses = categorie("Courses", CategoryKind::Depense);
        let loisirs = categorie("Loisirs", CategoryKind::Depense);
        let index = index(&[&courses, &loisirs]);

        let previsionnel = calculer_previsionnel(
            0,
            HashMap::from([(courses.id.0, 10_000), (loisirs.id.0, 50_000)]),
            HashMap::new(),
            &repartition(vec![]),
            &repartition(vec![]),
            &index,
        );

        let noms: Vec<String> = previsionnel
            .lignes
            .iter()
            .filter_map(|l| l.category.as_ref().map(|c| c.name.clone()))
            .collect();
        assert_eq!(noms, vec!["Loisirs", "Courses"]);
    }
}
