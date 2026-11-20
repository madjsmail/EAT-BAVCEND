// Polymorphic attachments: metadata in PostgreSQL, bytes in Garage.

pub mod dto;
pub mod handlers;
pub mod repo;

use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::get;

use crate::config::database::Database;

pub fn router() -> Router<Arc<Database>> {
    Router::new()
        .route(
            "/attachments",
            get(handlers::list_attachments).post(handlers::upload_attachment),
        )
        .route(
            "/attachments/{id}",
            axum::routing::delete(handlers::delete_attachment),
        )
        .route("/attachments/{id}/url", get(handlers::download_url))
        .layer(DefaultBodyLimit::max(repo::MAX_UPLOAD_BYTES))
}
