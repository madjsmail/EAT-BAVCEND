// Executive dashboard handlers. RBAC: `dashboard.read` on every endpoint.

use std::sync::Arc;

use axum::extract::State;

use crate::config::database::{Database, DatabaseTrait};
use crate::error::api_error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::rbac;
use crate::modules::dashboard::dto::*;
use crate::modules::dashboard::repo;
use crate::modules::i18n::Locale;
use crate::response::api_response::ApiResponse;

pub async fn asset_status(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
) -> Result<ApiResponse<Vec<StatusCount>>, ApiError> {
    rbac::require(&ctx, "dashboard.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let rows = repo::asset_status(db.get_pool(), ctx.organization_id(), &locale).await?;
    Ok(ApiResponse::success(rows))
}

pub async fn expirations(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
) -> Result<ApiResponse<ExpirationsResponse>, ApiError> {
    rbac::require(&ctx, "dashboard.read")?;
    let rows = repo::expirations(db.get_pool(), ctx.organization_id()).await?;
    Ok(ApiResponse::success(rows))
}

pub async fn low_stock(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
) -> Result<ApiResponse<Vec<LowStockItem>>, ApiError> {
    rbac::require(&ctx, "dashboard.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let rows = repo::low_stock(db.get_pool(), ctx.organization_id(), &locale).await?;
    Ok(ApiResponse::success(rows))
}

pub async fn fleet_value(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
) -> Result<ApiResponse<FleetValueResponse>, ApiError> {
    rbac::require(&ctx, "dashboard.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let rows = repo::fleet_value(db.get_pool(), ctx.organization_id(), &locale).await?;
    Ok(ApiResponse::success(rows))
}
