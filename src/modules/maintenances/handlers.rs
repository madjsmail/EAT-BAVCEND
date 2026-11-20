// Maintenance handlers: thin wrappers that validate, check RBAC and delegate.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use uuid::Uuid;

use crate::config::database::{Database, DatabaseTrait};
use crate::error::api_error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::{client_ip, rbac, user_agent};
use crate::modules::i18n::Locale;
use crate::modules::maintenances::dto::*;
use crate::modules::maintenances::repo;
use crate::response::api_response::ApiResponse;

fn maintenance_response(row: repo::MaintenanceRow) -> MaintenanceResponse {
    MaintenanceResponse {
        id: row.id,
        asset_id: row.asset_id,
        asset_tag: row.asset_tag,
        asset_name: row.asset_name,
        title: row.title,
        description: row.description,
        maintenance_type: row.maintenance_type,
        status: row.status,
        priority: row.priority,
        supplier_id: row.supplier_id,
        supplier_name: row.supplier_name,
        rma_reference: row.rma_reference,
        assigned_to_user_id: row.assigned_to_user_id,
        assigned_to_user_name: row.assigned_to_user_name,
        previous_status_id: row.previous_status_id,
        previous_status_name: row.previous_status_name,
        reported_at: row.reported_at,
        scheduled_at: row.scheduled_at,
        started_at: row.started_at,
        completed_at: row.completed_at,
        completion_notes: row.completion_notes,
        cost: row.cost,
        currency: row.currency,
        created_by_user_id: row.created_by_user_id,
        created_by_user_name: row.created_by_user_name,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

pub async fn list_maintenances(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Query(query): Query<MaintenanceListQuery>,
) -> Result<ApiResponse<Vec<MaintenanceResponse>>, ApiError> {
    rbac::require(&ctx, "maintenances.read")?;
    let locale = Locale::default().resolve(&ctx.user.preferred_locale);
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(25).clamp(1, 100);

    let (rows, total) = repo::list_maintenances(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        &query,
        page,
        per_page,
    )
    .await?;
    Ok(ApiResponse::paginated(
        rows.into_iter().map(maintenance_response).collect(),
        page,
        per_page,
        total as u64,
    ))
}

pub async fn get_maintenance(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<MaintenanceResponse>, ApiError> {
    rbac::require(&ctx, "maintenances.read")?;
    let locale = Locale::default().resolve(&ctx.user.preferred_locale);
    let row = repo::get_maintenance(db.get_pool(), ctx.organization_id(), &locale, id).await?;
    Ok(ApiResponse::success(maintenance_response(row)))
}

pub async fn create_maintenance(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    headers: HeaderMap,
    Json(payload): Json<CreateMaintenanceRequest>,
) -> Result<ApiResponse<MaintenanceResponse>, ApiError> {
    rbac::require(&ctx, "maintenances.write")?;
    let locale = Locale::default().resolve(&ctx.user.preferred_locale);
    let ip = client_ip(&headers);
    let ua = user_agent(&headers);
    let row = repo::create_maintenance(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        ctx.user.id,
        &ip,
        ua.as_deref(),
        &payload,
    )
    .await?;
    Ok(ApiResponse::success_with_message(
        maintenance_response(row),
        "Maintenance ticket created",
    ))
}

pub async fn update_maintenance(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateMaintenanceRequest>,
) -> Result<ApiResponse<MaintenanceResponse>, ApiError> {
    rbac::require(&ctx, "maintenances.write")?;
    let locale = Locale::default().resolve(&ctx.user.preferred_locale);
    let ip = client_ip(&headers);
    let ua = user_agent(&headers);
    let row = repo::update_maintenance(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        ctx.user.id,
        &ip,
        ua.as_deref(),
        id,
        &payload,
    )
    .await?;
    Ok(ApiResponse::success(maintenance_response(row)))
}

pub async fn start_maintenance(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<MaintenanceResponse>, ApiError> {
    rbac::require(&ctx, "maintenances.write")?;
    let locale = Locale::default().resolve(&ctx.user.preferred_locale);
    let ip = client_ip(&headers);
    let ua = user_agent(&headers);
    let row = repo::start_maintenance(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        ctx.user.id,
        &ip,
        ua.as_deref(),
        id,
    )
    .await?;
    Ok(ApiResponse::success_with_message(
        maintenance_response(row),
        "Maintenance started",
    ))
}

pub async fn complete_maintenance(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<CompleteMaintenanceRequest>,
) -> Result<ApiResponse<MaintenanceResponse>, ApiError> {
    rbac::require(&ctx, "maintenances.write")?;
    let locale = Locale::default().resolve(&ctx.user.preferred_locale);
    let ip = client_ip(&headers);
    let ua = user_agent(&headers);
    let row = repo::close_maintenance(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        ctx.user.id,
        &ip,
        ua.as_deref(),
        id,
        true,
        payload.completion_notes.as_deref(),
        payload.cost.flatten(),
        payload.restore_status_id,
    )
    .await?;
    Ok(ApiResponse::success_with_message(
        maintenance_response(row),
        "Maintenance completed",
    ))
}

pub async fn cancel_maintenance(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<CancelMaintenanceRequest>,
) -> Result<ApiResponse<MaintenanceResponse>, ApiError> {
    rbac::require(&ctx, "maintenances.write")?;
    let locale = Locale::default().resolve(&ctx.user.preferred_locale);
    let ip = client_ip(&headers);
    let ua = user_agent(&headers);
    let row = repo::close_maintenance(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        ctx.user.id,
        &ip,
        ua.as_deref(),
        id,
        false,
        payload.reason.as_deref(),
        None,
        None,
    )
    .await?;
    Ok(ApiResponse::success_with_message(
        maintenance_response(row),
        "Maintenance cancelled",
    ))
}

pub async fn delete_maintenance(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<String>, ApiError> {
    rbac::require(&ctx, "maintenances.write")?;
    let ip = client_ip(&headers);
    let ua = user_agent(&headers);
    repo::delete_maintenance(
        db.get_pool(),
        ctx.organization_id(),
        ctx.user.id,
        &ip,
        ua.as_deref(),
        id,
    )
    .await?;
    Ok(ApiResponse::success_with_message(
        id.to_string(),
        "Maintenance ticket deleted",
    ))
}
