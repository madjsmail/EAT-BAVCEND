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

// ------------------------------------------------ schedule-driven workflow ----

fn work_order_board_item(row: repo::WorkOrderRow) -> WorkOrderBoardItem {
    WorkOrderBoardItem {
        id: row.id,
        asset: AssetRef {
            id: row.asset_id,
            name: row.asset_name,
        },
        work_order_type: row.work_order_type,
        status: row.status,
        assigned_to: row.assigned_to,
        due_date: row.due_date,
        cost: row.cost,
        created_at: row.created_at,
    }
}

fn work_order_event_response(row: repo::WorkOrderEventRow) -> WorkOrderEventResponse {
    WorkOrderEventResponse {
        id: row.id,
        event_type: row.event_type,
        actor_id: row.actor_id,
        actor_name: row.actor_name,
        payload: row.payload,
        created_at: row.created_at,
    }
}

fn work_order_detail_response(
    row: repo::WorkOrderRow,
    events: Vec<repo::WorkOrderEventRow>,
) -> WorkOrderDetailResponse {
    WorkOrderDetailResponse {
        id: row.id,
        schedule_id: row.schedule_id,
        asset: AssetRef {
            id: row.asset_id,
            name: row.asset_name,
        },
        work_order_type: row.work_order_type,
        status: row.status,
        assigned_to: row.assigned_to,
        assigned_to_name: row.assigned_to_name,
        due_date: row.due_date,
        completed_at: row.completed_at,
        cost: row.cost,
        notes: row.notes,
        created_at: row.created_at,
        updated_at: row.updated_at,
        events: events.into_iter().map(work_order_event_response).collect(),
    }
}

fn schedule_response(row: repo::MaintenanceScheduleRow) -> MaintenanceScheduleResponse {
    MaintenanceScheduleResponse {
        id: row.id,
        asset_id: row.asset_id,
        frequency_type: row.frequency_type,
        interval: row.interval,
        next_due_at: row.next_due_at,
        next_due_operating_hours: row.next_due_operating_hours,
        is_active: row.is_active,
        notes: row.notes,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

async fn load_work_order_detail(
    pool: &sqlx::PgPool,
    org: Uuid,
    id: Uuid,
) -> Result<WorkOrderDetailResponse, ApiError> {
    let row = repo::get_work_order(pool, org, id).await?;
    let events = repo::list_work_order_events(pool, org, id).await?;
    Ok(work_order_detail_response(row, events))
}

pub async fn list_work_orders(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Query(query): Query<WorkOrderListQuery>,
) -> Result<ApiResponse<Vec<WorkOrderBoardItem>>, ApiError> {
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(25).clamp(1, 100);
    let (rows, total) =
        repo::list_work_orders(db.get_pool(), ctx.organization_id(), &query, page, per_page)
            .await?;
    Ok(ApiResponse::paginated(
        rows.into_iter().map(work_order_board_item).collect(),
        page,
        per_page,
        total as u64,
    ))
}

pub async fn get_work_order(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<WorkOrderDetailResponse>, ApiError> {
    let detail = load_work_order_detail(db.get_pool(), ctx.organization_id(), id).await?;
    Ok(ApiResponse::success(detail))
}

pub async fn create_work_order(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Json(payload): Json<CreateWorkOrderRequest>,
) -> Result<ApiResponse<WorkOrderDetailResponse>, ApiError> {
    let row = repo::create_work_order(db.get_pool(), ctx.organization_id(), ctx.user.id, &payload)
        .await?;
    let detail = load_work_order_detail(db.get_pool(), ctx.organization_id(), row.id).await?;
    Ok(ApiResponse::success_with_message(
        detail,
        "Work order created",
    ))
}

pub async fn update_work_order(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateWorkOrderRequest>,
) -> Result<ApiResponse<WorkOrderDetailResponse>, ApiError> {
    if payload.status.as_deref() == Some("done") {
        let current = repo::get_work_order(db.get_pool(), ctx.organization_id(), id).await?;
        let assigned = current.assigned_to == Some(ctx.user.id);
        let admin = repo::is_admin(db.get_pool(), ctx.organization_id(), ctx.user.id).await?;
        if !assigned && !admin {
            return Err(ApiError::Forbidden(
                "only the assignee or an administrator can complete this work order".to_string(),
            ));
        }
    }

    let row = repo::update_work_order(
        db.get_pool(),
        ctx.organization_id(),
        ctx.user.id,
        id,
        &payload,
    )
    .await?;
    let detail = load_work_order_detail(db.get_pool(), ctx.organization_id(), row.id).await?;
    Ok(ApiResponse::success(detail))
}

pub async fn add_work_order_event(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(payload): Json<AddWorkOrderEventRequest>,
) -> Result<ApiResponse<WorkOrderEventResponse>, ApiError> {
    let event = repo::add_work_order_note(
        db.get_pool(),
        ctx.organization_id(),
        ctx.user.id,
        id,
        &payload,
    )
    .await?;
    Ok(ApiResponse::success_with_message(
        work_order_event_response(event),
        "Work order event added",
    ))
}

pub async fn report_breakdown(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(asset_id): Path<Uuid>,
    Json(payload): Json<ReportBreakdownRequest>,
) -> Result<ApiResponse<WorkOrderDetailResponse>, ApiError> {
    let row = repo::report_breakdown(
        db.get_pool(),
        ctx.organization_id(),
        ctx.user.id,
        asset_id,
        payload.notes,
    )
    .await?;
    let detail = load_work_order_detail(db.get_pool(), ctx.organization_id(), row.id).await?;
    Ok(ApiResponse::success_with_message(
        detail,
        "Breakdown reported",
    ))
}

pub async fn list_maintenance_schedules(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(asset_id): Path<Uuid>,
) -> Result<ApiResponse<Vec<MaintenanceScheduleResponse>>, ApiError> {
    let rows = repo::list_schedules(db.get_pool(), ctx.organization_id(), asset_id).await?;
    Ok(ApiResponse::success(
        rows.into_iter().map(schedule_response).collect(),
    ))
}

pub async fn create_maintenance_schedule(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(asset_id): Path<Uuid>,
    Json(payload): Json<CreateMaintenanceScheduleRequest>,
) -> Result<ApiResponse<MaintenanceScheduleResponse>, ApiError> {
    let row = repo::create_schedule(
        db.get_pool(),
        ctx.organization_id(),
        ctx.user.id,
        asset_id,
        &payload,
    )
    .await?;
    Ok(ApiResponse::success_with_message(
        schedule_response(row),
        "Maintenance schedule created",
    ))
}

pub async fn update_maintenance_schedule(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateMaintenanceScheduleRequest>,
) -> Result<ApiResponse<MaintenanceScheduleResponse>, ApiError> {
    let row = repo::update_schedule(
        db.get_pool(),
        ctx.organization_id(),
        ctx.user.id,
        id,
        &payload,
    )
    .await?;
    Ok(ApiResponse::success(schedule_response(row)))
}

pub async fn deactivate_maintenance_schedule(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<String>, ApiError> {
    repo::deactivate_schedule(db.get_pool(), ctx.organization_id(), ctx.user.id, id).await?;
    Ok(ApiResponse::success_with_message(
        id.to_string(),
        "Maintenance schedule deactivated",
    ))
}

pub async fn maintenance_history(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(asset_id): Path<Uuid>,
    Query(query): Query<MaintenanceHistoryQuery>,
) -> Result<ApiResponse<Vec<MaintenanceHistoryItem>>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let items = repo::maintenance_history(
        db.get_pool(),
        ctx.organization_id(),
        asset_id,
        query.cursor,
        limit,
    )
    .await?;
    let next_cursor = if items.len() == limit as usize {
        items.last().map(|item| item.occurred_at.to_rfc3339())
    } else {
        None
    };
    Ok(ApiResponse::paginated_with_cursor(
        items,
        1,
        limit,
        limit as u64,
        next_cursor,
    ))
}
