// Universal asset registry: CRUD, quick-scan, custody transitions, labels.

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
            "/assets",
            get(handlers::list_assets).post(handlers::create_asset),
        )
        .route("/assets/scan", get(handlers::scan_asset))
        .route(
            "/assets/{id}",
            get(handlers::get_asset)
                .patch(handlers::update_asset)
                .delete(handlers::delete_asset),
        )
        .route("/assets/{id}/360", get(handlers::asset_360))
        .route("/assets/{id}/label", get(handlers::asset_label))
        .route("/assets/{id}/assign", post(handlers::assign_asset))
        .route("/assets/{id}/return", post(handlers::return_asset))
        .route("/assets/{id}/transfer", post(handlers::transfer_asset))
        .route("/assets/{id}/assignments", get(handlers::list_assignments))
}
