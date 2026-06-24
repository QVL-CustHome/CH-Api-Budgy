use crate::handlers;
use crate::state::AppState;
use axum::Router;
use axum::routing::get;

pub const API_VERSION_PREFIX: &str = "/v1";

pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(operational_routes())
        .nest(API_VERSION_PREFIX, public_routes())
        .with_state(state)
}

fn operational_routes() -> Router<AppState> {
    Router::new().route("/health", get(handlers::health::health))
}

fn public_routes() -> Router<AppState> {
    Router::new()
        .route("/me", get(handlers::me::me))
        .route("/accounts", get(handlers::comptes::list_accounts))
        .route(
            "/accounts/{account_id}/balance",
            get(handlers::comptes::get_account_balance),
        )
        .route(
            "/transactions",
            get(handlers::transactions::list_transactions),
        )
}
