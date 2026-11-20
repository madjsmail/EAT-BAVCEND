use crate::error::api_error::ApiError;
use crate::middleware::auth::AuthContext;

/// Permission guard: `rbac::require(&ctx, "invitations.create")?`.
pub fn require(ctx: &AuthContext, permission: &str) -> Result<(), ApiError> {
    if ctx.has(permission) {
        Ok(())
    } else {
        Err(ApiError::Forbidden(permission.to_string()))
    }
}
