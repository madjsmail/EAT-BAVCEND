// Executive dashboard aggregations.

pub mod dto;
pub mod handlers;
pub mod repo;

use std::sync::Arc;

use axum::Router;
use axum::routing::get;

use crate::config::database::Database;

pub fn router() -> Router<Arc<Database>> {
    Router::new()
        .route("/dashboard/asset-status", get(handlers::asset_status))
        .route("/dashboard/expirations", get(handlers::expirations))
        .route("/dashboard/low-stock", get(handlers::low_stock))
        .route("/dashboard/fleet-value", get(handlers::fleet_value))
}
