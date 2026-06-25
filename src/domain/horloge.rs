use chrono::{DateTime, Utc};
use std::sync::Arc;

pub trait Horloge: Send + Sync {
    fn maintenant(&self) -> DateTime<Utc>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HorlogeSysteme;

impl Horloge for HorlogeSysteme {
    fn maintenant(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

impl Horloge for Arc<dyn Horloge> {
    fn maintenant(&self) -> DateTime<Utc> {
        self.as_ref().maintenant()
    }
}
