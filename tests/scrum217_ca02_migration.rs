mod common;

use common::DisposableDb;

macro_rules! require_db {
    () => {
        match DisposableDb::create().await {
            Some(db) => db,
            None => {
                eprintln!(
                    "SCRUM-217 CA-02 ignoré : variable {} absente (Postgres jetable requis)",
                    common::ENV_ADMIN_URL
                );
                return;
            }
        }
    };
}

#[tokio::test]
async fn migration_cree_le_schema_budgy() {
    let db = require_db!();
    db.migrate().await;

    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.schemata WHERE schema_name = 'budgy')",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert!(exists, "le schéma budgy doit être créé");

    db.destroy().await;
}

#[tokio::test]
async fn etat_des_migrations_tracable() {
    let db = require_db!();
    db.migrate().await;

    let (version, success): (i64, bool) =
        sqlx::query_as("SELECT version, success FROM _sqlx_migrations ORDER BY version LIMIT 1")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(version, 1, "la migration 0001 doit être tracée");
    assert!(success, "la migration 0001 doit être marquée appliquée");

    db.destroy().await;
}

#[tokio::test]
async fn migration_idempotente_ne_se_rejoue_pas() {
    let db = require_db!();
    db.migrate().await;

    let avant: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
        .fetch_one(&db.pool)
        .await
        .unwrap();

    db::migrate_again(&db.pool).await;

    let apres: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        apres, avant,
        "une seconde migration ne doit pas rejouer ni ajouter de ligne"
    );

    db.destroy().await;
}

mod db {
    use ch_api_budgy::db::{self, Db};

    pub async fn migrate_again(pool: &Db) {
        db::migrate(pool)
            .await
            .expect("la migration doit être idempotente (rejeu sans erreur)");
    }
}
