use crate::extract::BudgyUser;
use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct MeResponse {
    owner_id: String,
    roles: Vec<String>,
}

pub async fn me(user: BudgyUser) -> Json<MeResponse> {
    Json(MeResponse {
        owner_id: user.owner_id().to_string(),
        roles: user.roles().to_vec(),
    })
}
