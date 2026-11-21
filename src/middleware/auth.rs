use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::config::database::{Database, DatabaseTrait};
use crate::entity::{Session, User};
use crate::error::api_error::ApiError;
use crate::repository::session;
use crate::service;

/// Authenticated request context. `organization_id` always comes from the
/// session row, so a client cannot select a tenant.
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub user: User,
    pub session: Session,
    pub permissions: Vec<String>,
}

impl AuthContext {
    pub fn organization_id(&self) -> uuid::Uuid {
        self.session.organization_id
    }

    pub fn has(&self, permission: &str) -> bool {
        self.permissions.iter().any(|p| p == permission)
    }
}

impl FromRequestParts<std::sync::Arc<Database>> for AuthContext {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &std::sync::Arc<Database>,
    ) -> Result<Self, Self::Rejection> {
        let pool = state.get_pool();
        let token =
            service::session_token_from_headers(&parts.headers).ok_or(ApiError::Unauthorized)?;
        let token_hash = service::hash_token(&token);

        let (session_row, user) = session::find_live_by_token(pool, &token_hash)
            .await?
            .ok_or(ApiError::Unauthorized)?;

        let permissions = crate::repository::role::permissions_for_user(
            pool,
            session_row.organization_id,
            user.id,
        )
        .await?;

        // Activity timestamp is best-effort: never block a valid request on it.
        if let Err(e) = crate::repository::session::touch_last_seen(pool, session_row.id).await {
            log::warn!("Could not refresh session last_seen_at: {e}");
        }

        Ok(Self {
            user,
            session: session_row,
            permissions,
        })
    }
}
