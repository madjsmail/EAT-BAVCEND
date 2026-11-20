// Software licenses: seat-limited allocation and expiry alerts.

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
            "/licenses",
            get(handlers::list_licenses).post(handlers::create_license),
        )
        .route("/licenses/alerts", get(handlers::expiry_alerts))
        .route(
            "/licenses/{id}",
            get(handlers::get_license)
                .patch(handlers::update_license)
                .delete(handlers::delete_license),
        )
        .route("/licenses/{id}/seats", get(handlers::list_seats))
        .route(
            "/licenses/{id}/seats/allocate",
            post(handlers::allocate_seat),
        )
        .route(
            "/licenses/{id}/seats/{seat_id}/revoke",
            post(handlers::revoke_seat),
        )
}
