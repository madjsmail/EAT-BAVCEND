// Audit Log Explorer (read-only).

pub mod dto;
pub mod handlers;
pub mod repo;

use std::sync::Arc;

use axum::Router;
use axum::routing::get;

use crate::config::database::Database;

pub fn router() -> Router<Arc<Database>> {
    Router::new().route("/audit-logs", get(handlers::list_audit_logs))
}
