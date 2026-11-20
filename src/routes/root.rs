use crate::config::database::{Database, DatabaseTrait};
use crate::response::api_response::{ApiResponse, ErrorDetail};
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, IntoMakeService};
use axum::Router;
use log::error;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

async fn health(State(db): State<Arc<Database>>) -> ApiResponse<String> {
    match sqlx::query("SELECT 1").execute(db.get_pool()).await {
        Ok(_) => ApiResponse::success("Healthy...".to_string()),
        Err(e) => {
            error!("Health check failed: {}", e);
            ApiResponse::error(
                StatusCode::SERVICE_UNAVAILABLE.as_u16(),
                "Service unavailable",
                vec![ErrorDetail::new("db_error", e.to_string())],
            )
        }
    }
}

pub fn routes(db_conn: Arc<Database>) -> IntoMakeService<Router> {
    let api = Router::<Arc<Database>>::new().route("/health", get(health));

    Router::<Arc<Database>>::new()
        .nest("/v1/api", api)
        .with_state(db_conn)
        .layer(TraceLayer::new_for_http())
        .into_make_service()
}