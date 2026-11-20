// Maintenance & RMA tracking.

pub mod dto;
pub mod handlers;
pub mod repo;

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};

use crate::config::database::Database;

pub fn router() -> Router<Arc<Database>> {
    Router::new()
        .route(
            "/maintenances",
            get(handlers::list_maintenances).post(handlers::create_maintenance),
        )
        .route(
            "/maintenances/{id}",
            get(handlers::get_maintenance)
                .patch(handlers::update_maintenance)
                .delete(handlers::delete_maintenance),
        )
        .route("/maintenances/{id}/start", post(handlers::start_maintenance))
        .route(
            "/maintenances/{id}/complete",
            post(handlers::complete_maintenance),
        )
        .route(
            "/maintenances/{id}/cancel",
            post(handlers::cancel_maintenance),
        )
}
