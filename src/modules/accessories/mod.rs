// Bulk accessories with quantity-tracked assignments.

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
            "/accessories",
            get(handlers::list_accessories).post(handlers::create_accessory),
        )
        .route(
            "/accessories/{id}",
            get(handlers::get_accessory)
                .patch(handlers::update_accessory)
                .delete(handlers::delete_accessory),
        )
        .route(
            "/accessories/{id}/assignments",
            get(handlers::list_assignments),
        )
        .route("/accessories/{id}/assign", post(handlers::assign_accessory))
        .route(
            "/accessories/{id}/assignments/{assignment_id}/return",
            post(handlers::return_accessory),
        )
}
