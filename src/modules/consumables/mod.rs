// Consumables: stock levels, low-stock alerts and the atomic ledger.

pub mod dto;
pub mod handlers;
pub mod repo;

use std::sync::Arc;

use axum::Router;
use axum::routing::get;

use crate::config::database::Database;

pub fn router() -> Router<Arc<Database>> {
    Router::new()
        .route(
            "/consumables",
            get(handlers::list_consumables).post(handlers::create_consumable),
        )
        .route("/consumables/alerts", get(handlers::low_stock_alerts))
        .route(
            "/consumables/{id}",
            get(handlers::get_consumable)
                .patch(handlers::update_consumable)
                .delete(handlers::delete_consumable),
        )
        .route(
            "/consumables/{id}/transactions",
            get(handlers::list_transactions).post(handlers::create_transaction),
        )
}
