// Organization settings: profile of the tenant and logo metadata.

pub mod dto;
pub mod handlers;
pub mod repo;

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};

use crate::config::database::Database;

pub fn router() -> Router<Arc<Database>> {
    Router::new()
        .route("/organization", get(handlers::get).patch(handlers::update))
        .route(
            "/organization/logo/upload-url",
            post(handlers::logo_upload_url),
        )
}
