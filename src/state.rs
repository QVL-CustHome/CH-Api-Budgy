use crate::config::Settings;
use crate::db::Db;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
}

impl AppState {
    pub fn new(_settings: &Settings, db: Db) -> Self {
        Self { db }
    }
}
