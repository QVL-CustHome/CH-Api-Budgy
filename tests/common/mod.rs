#![allow(dead_code)]

use ch_api_budgy::db;
use ch_api_budgy::db::Db;
use sqlx::{Connection, Executor, PgConnection};
use uuid::Uuid;

pub const ENV_ADMIN_URL: &str = "BUDGY_TEST_DATABASE_URL";

pub struct DisposableDb {
    admin_url: String,
    db_name: String,
    pub pool: Db,
}

impl DisposableDb {
    pub async fn create() -> Option<Self> {
        let admin_url = std::env::var(ENV_ADMIN_URL).ok()?;
        let db_name = format!("budgy_it_{}", Uuid::new_v4().simple());

        let mut admin = match PgConnection::connect(&admin_url).await {
            Ok(admin) => admin,
            Err(erreur) => {
                eprintln!(
                    "{ENV_ADMIN_URL} défini mais connexion à la base d'administration impossible ({erreur}) : tests d'intégration ignorés"
                );
                return None;
            }
        };

        if let Err(erreur) = admin
            .execute(format!("CREATE DATABASE \"{db_name}\"").as_str())
            .await
        {
            eprintln!(
                "création de la base jetable refusée ({erreur}) : le rôle doit disposer du privilège CREATEDB, tests d'intégration ignorés"
            );
            admin.close().await.ok();
            return None;
        }
        admin.close().await.ok();

        let db_url = replace_database(&admin_url, &db_name);
        let pool = match db::connect(&db_url).await {
            Ok(pool) => pool,
            Err(erreur) => {
                eprintln!(
                    "connexion à la base jetable impossible ({erreur}) : tests d'intégration ignorés"
                );
                return None;
            }
        };

        Some(Self {
            admin_url,
            db_name,
            pool,
        })
    }

    pub async fn migrate(&self) {
        db::migrate(&self.pool)
            .await
            .expect("migrations en échec sur la base jetable");
    }

    pub async fn destroy(self) {
        let Self {
            admin_url,
            db_name,
            pool,
        } = self;
        pool.close().await;

        if let Ok(mut admin) = PgConnection::connect(&admin_url).await {
            let _ = admin
                .execute(
                    format!(
                        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
                         WHERE datname = '{db_name}' AND pid <> pg_backend_pid()"
                    )
                    .as_str(),
                )
                .await;
            let _ = admin
                .execute(format!("DROP DATABASE IF EXISTS \"{db_name}\"").as_str())
                .await;
            admin.close().await.ok();
        }
    }
}

fn replace_database(url: &str, db_name: &str) -> String {
    match url.rfind('/') {
        Some(idx) => {
            let base = &url[..idx];
            let query = url[idx + 1..]
                .find('?')
                .map(|q| &url[idx + 1 + q..])
                .unwrap_or("");
            format!("{base}/{db_name}{query}")
        }
        None => format!("{url}/{db_name}"),
    }
}
