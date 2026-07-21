use crate::handlers;
use crate::state::AppState;
use axum::Router;
use axum::routing::{get, post, put};

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
            "/accounts/{account_id}",
            get(handlers::comptes::get_account),
        )
        .route(
            "/accounts/{account_id}/transactions",
            get(handlers::comptes::list_account_transactions),
        )
        .route(
            "/accounts/{account_id}/transactions/{transaction_id}/category",
            put(handlers::comptes::categorize_transaction),
        )
        .route(
            "/categories",
            get(handlers::categories::list_categories).post(handlers::categories::create_category),
        )
        .route(
            "/categories/{category_id}",
            put(handlers::categories::update_category)
                .delete(handlers::categories::delete_category),
        )
        .route("/banks", get(handlers::banques::list_banks))
        .route(
            "/consents",
            get(handlers::banques::list_consents).post(handlers::banques::create_consent),
        )
        .route(
            "/consents/callback",
            post(handlers::banques::complete_consent),
        )
        .route(
            "/consents/{consent_id}/renew",
            post(handlers::banques::renew_consent),
        )
}
