// Product catalog (models) — `specs_json` / `custom_fields_json` keep
// per-asset-type attributes without new columns.

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
            "/products",
            get(handlers::list_products).post(handlers::create_product),
        )
        .route(
            "/products/{id}",
            get(handlers::get_product)
                .patch(handlers::update_product)
                .delete(handlers::delete_product),
        )
}
