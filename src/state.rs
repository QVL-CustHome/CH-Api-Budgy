use crate::config::Settings;

#[derive(Clone)]
pub struct AppState {}

impl AppState {
    pub fn new(_settings: &Settings) -> Self {
        Self {}
    }
}
