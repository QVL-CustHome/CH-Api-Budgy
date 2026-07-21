mod common;

use ch_api_budgy::domain::category::{Category, CategoryKind};
use ch_api_budgy::domain::ports::lecture::CategoriesReadRepository;
use ch_api_budgy::repository::categories::SqlxCategoriesRepository;
use common::DisposableDb;

macro_rules! db_or_skip {
    () => {
        match DisposableDb::create().await {
            Some(db) => {
                db.migrate().await;
                db
            }
            None => {
                eprintln!("BUDGY_TEST_DATABASE_URL absente : test ignoré");
                return;
            }
        }
    };
}

async fn lister(db: &DisposableDb) -> Vec<Category> {
    SqlxCategoriesRepository::new(db.pool.clone())
        .lister()
        .await
        .expect("liste des catégories")
}

fn kind_de(categories: &[Category], nom: &str) -> CategoryKind {
    categories
        .iter()
        .find(|c| c.name == nom)
        .unwrap_or_else(|| panic!("catégorie « {nom} » absente du seed"))
        .kind
}

#[tokio::test]
async fn seed_installe_dix_categories_par_defaut() {
    let db = db_or_skip!();

    let categories = lister(&db).await;

    assert_eq!(categories.len(), 10);

    db.destroy().await;
}

#[tokio::test]
async fn seed_repartit_deux_revenus_et_huit_depenses() {
    let db = db_or_skip!();

    let categories = lister(&db).await;
    let revenus = categories
        .iter()
        .filter(|c| c.kind == CategoryKind::Revenu)
        .count();
    let depenses = categories
        .iter()
        .filter(|c| c.kind == CategoryKind::Depense)
        .count();

    assert_eq!(revenus, 2);
    assert_eq!(depenses, 8);

    db.destroy().await;
}

#[tokio::test]
async fn liste_est_triee_par_kind_puis_name() {
    let db = db_or_skip!();

    let categories = lister(&db).await;
    let ordre_recu: Vec<(&str, &str)> = categories
        .iter()
        .map(|c| (c.kind.as_str(), c.name.as_str()))
        .collect();
    let mut ordre_attendu = ordre_recu.clone();
    ordre_attendu.sort();

    assert_eq!(ordre_recu, ordre_attendu);

    db.destroy().await;
}

#[tokio::test]
async fn seed_contient_les_exemples_de_l_ac_avec_le_bon_type() {
    let db = db_or_skip!();

    let categories = lister(&db).await;

    assert_eq!(kind_de(&categories, "Salaire"), CategoryKind::Revenu);
    assert_eq!(kind_de(&categories, "Loyer"), CategoryKind::Depense);
    assert_eq!(kind_de(&categories, "Courses"), CategoryKind::Depense);

    db.destroy().await;
}
