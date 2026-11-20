use crate::config::database::{Database, DatabaseTrait};
use crate::handler;
use crate::response::api_response::{ApiResponse, ErrorDetail};
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{delete, get, patch, post};
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

/// Router factory shared by the server and the integration tests.
pub fn app(db_conn: Arc<Database>) -> Router {
    let api = Router::<Arc<Database>>::new()
        .route("/health", get(health))
        // Public (invite-only) auth surface.
        .route("/auth/login", post(handler::login))
        .route("/invite/{token}", get(handler::preview_invitation))
        .route("/invitations/accept", post(handler::accept_invitation))
        // Authenticated surface.
        .route("/auth/logout", post(handler::logout))
        .route("/auth/me", get(handler::me))
        .route(
            "/invitations",
            post(handler::create_invitation).get(handler::list_invitations),
        )
        .route("/invitations/{id}", delete(handler::revoke_invitation))
        .route("/roles", get(handler::list_roles))
        .route(
            "/sessions",
            get(handler::list_sessions).delete(handler::revoke_all_sessions),
        )
        .route("/sessions/{id}", delete(handler::revoke_session))
        .route("/users/{id}/status", patch(handler::update_user_status))
        // Milestone 2: lookups, catalog, assets, custody.
        .merge(crate::modules::lookups::router())
        .merge(crate::modules::products::router())
        .merge(crate::modules::assets::router())
        // Milestone 3: accessories, consumables, licenses.
        .merge(crate::modules::accessories::router())
        .merge(crate::modules::consumables::router())
        .merge(crate::modules::licenses::router());

    Router::<Arc<Database>>::new()
        .nest("/v1/api", api)
        .with_state(db_conn)
        .layer(TraceLayer::new_for_http())
}

pub fn routes(db_conn: Arc<Database>) -> axum::routing::IntoMakeService<Router> {
    app(db_conn).into_make_service()
}
