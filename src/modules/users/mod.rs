// Employee directory: CRUD, hierarchy, bulk import, offboarding.

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
            "/users",
            get(handlers::list_users).post(handlers::create_user),
        )
        .route("/users/import", post(handlers::import_users))
        .route(
            "/users/{id}",
            get(handlers::get_user)
                .patch(handlers::update_user)
                .delete(handlers::delete_user),
        )
        .route("/users/{id}/direct-reports", get(handlers::direct_reports))
        .route("/users/{id}/offboard", post(handlers::offboard_user))
        .route("/users/{id}/pending-returns", get(handlers::pending_returns))
}
