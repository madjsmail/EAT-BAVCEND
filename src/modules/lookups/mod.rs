// Lookup CRUD: categories, manufacturers, statuses, departments, locations,
// suppliers and depreciation models.

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
            "/lookups/categories",
            get(handlers::list_categories).post(handlers::create_category),
        )
        .route(
            "/lookups/categories/{id}",
            get(handlers::get_category)
                .patch(handlers::update_category)
                .delete(handlers::delete_category),
        )
        .route(
            "/lookups/manufacturers",
            get(handlers::list_manufacturers).post(handlers::create_manufacturer),
        )
        .route(
            "/lookups/manufacturers/{id}",
            get(handlers::get_manufacturer)
                .patch(handlers::update_manufacturer)
                .delete(handlers::delete_manufacturer),
        )
        .route(
            "/lookups/statuses",
            get(handlers::list_statuses).post(handlers::create_status),
        )
        .route(
            "/lookups/statuses/{id}",
            get(handlers::get_status)
                .patch(handlers::update_status)
                .delete(handlers::delete_status),
        )
        .route(
            "/lookups/departments",
            get(handlers::list_departments).post(handlers::create_department),
        )
        .route(
            "/lookups/departments/{id}",
            get(handlers::get_department)
                .patch(handlers::update_department)
                .delete(handlers::delete_department),
        )
        .route(
            "/lookups/locations",
            get(handlers::list_locations).post(handlers::create_location),
        )
        .route(
            "/lookups/locations/{id}",
            get(handlers::get_location)
                .patch(handlers::update_location)
                .delete(handlers::delete_location),
        )
        .route(
            "/lookups/suppliers",
            get(handlers::list_suppliers).post(handlers::create_supplier),
        )
        .route(
            "/lookups/suppliers/{id}",
            get(handlers::get_supplier)
                .patch(handlers::update_supplier)
                .delete(handlers::delete_supplier),
        )
        .route(
            "/lookups/depreciation-models",
            get(handlers::list_depreciation_models).post(handlers::create_depreciation_model),
        )
        .route(
            "/lookups/depreciation-models/{id}",
            get(handlers::get_depreciation_model)
                .patch(handlers::update_depreciation_model)
                .delete(handlers::delete_depreciation_model),
        )
}
