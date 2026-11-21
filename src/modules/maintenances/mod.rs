// Maintenance & RMA tracking.

pub mod dto;
pub mod handlers;
pub mod repo;

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, patch, post};

use crate::config::database::Database;

pub use repo::run_once;

/// Convenience path for callers that prefer a scheduler namespace.
pub mod scheduler {
    pub use super::repo::run_once;
}

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
        .route(
            "/maintenances/{id}/start",
            post(handlers::start_maintenance),
        )
        .route(
            "/maintenances/{id}/complete",
            post(handlers::complete_maintenance),
        )
        .route(
            "/maintenances/{id}/cancel",
            post(handlers::cancel_maintenance),
        )
        // Schedule-driven work orders.
        .route(
            "/work-orders",
            get(handlers::list_work_orders).post(handlers::create_work_order),
        )
        .route(
            "/work-orders/{id}",
            get(handlers::get_work_order).patch(handlers::update_work_order),
        )
        .route(
            "/work-orders/{id}/events",
            post(handlers::add_work_order_event),
        )
        .route(
            "/assets/{id}/report-breakdown",
            post(handlers::report_breakdown),
        )
        .route(
            "/assets/{id}/maintenance-schedules",
            get(handlers::list_maintenance_schedules).post(handlers::create_maintenance_schedule),
        )
        .route(
            "/assets/{id}/maintenance-history",
            get(handlers::maintenance_history),
        )
        .route(
            "/maintenance-schedules/{id}",
            patch(handlers::update_maintenance_schedule)
                .delete(handlers::deactivate_maintenance_schedule),
        )
}
