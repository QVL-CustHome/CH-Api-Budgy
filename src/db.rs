use sqlx::Executor;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Pool, Postgres};
use std::time::Duration;

pub type Db = Pool<Postgres>;

pub async fn connect(url: &str) -> Result<Db, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                conn.execute("SET search_path TO public").await?;
                Ok(())
            })
        })
        .connect(url)
        .await
}

pub async fn migrate(pool: &Db) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}
