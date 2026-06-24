use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ListResponse<T> {
    pub data: Vec<T>,
    pub total: u64,
}

impl<T> ListResponse<T> {
    pub fn new(data: Vec<T>, total: u64) -> Self {
        Self { data, total }
    }
}
