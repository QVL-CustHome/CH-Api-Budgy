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
async fn migration_cree_la_table_bank_credential_avec_les_colonnes_attendues() {
    let db = require_db!();
    db.migrate().await;

    let colonnes: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT column_name, data_type, is_nullable \
         FROM information_schema.columns \
         WHERE table_schema = 'budgy' AND table_name = 'bank_credential' \
         ORDER BY ordinal_position",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();

    let attendu = vec![
        ("id", "uuid", "NO"),
        ("owner_id", "text", "NO"),
        ("access_token", "bytea", "NO"),
        ("key_version", "smallint", "NO"),
        ("created_at", "timestamp with time zone", "NO"),
        ("updated_at", "timestamp with time zone", "NO"),
    ];
    let reel: Vec<(&str, &str, &str)> = colonnes
        .iter()
        .map(|(n, t, nul)| (n.as_str(), t.as_str(), nul.as_str()))
        .collect();
    assert_eq!(reel, attendu, "colonnes de budgy.bank_credential");

    db.destroy().await;
}

#[tokio::test]
async fn migration_pose_la_pk_sur_id() {
    let db = require_db!();
    db.migrate().await;

    let pk: Vec<String> = sqlx::query_scalar(
        "SELECT a.attname FROM pg_index i \
         JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY (i.indkey) \
         WHERE i.indrelid = 'budgy.bank_credential'::regclass AND i.indisprimary",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(pk, vec!["id".to_string()], "la PK doit porter sur id");

    db.destroy().await;
}

#[tokio::test]
async fn migration_cree_l_index_owner() {
    let db = require_db!();
    db.migrate().await;

    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_indexes \
         WHERE schemaname = 'budgy' AND tablename = 'bank_credential' \
         AND indexname = 'bank_credential_owner')",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert!(exists, "l'index bank_credential_owner doit exister");

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

    db::migrate_again(&db.pool).await;

    let appliquees: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        appliquees, 1,
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
