// Request/response shapes for maintenance tickets and RMA tracking.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::modules::common::double_option;

#[derive(Debug, Deserialize)]
pub struct MaintenanceListQuery {
    pub q: Option<String>,
    pub asset_id: Option<Uuid>,
    /// `open`, `in_progress`, `completed` or `cancelled`.
    pub status: Option<String>,
    pub maintenance_type: Option<String>,
    pub priority: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct CreateMaintenanceRequest {
    pub asset_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    /// `preventive`, `corrective` or `inspection`.
    pub maintenance_type: Option<String>,
    /// `low`, `medium`, `high` or `critical`.
    pub priority: Option<String>,
    pub supplier_id: Option<Uuid>,
    pub rma_reference: Option<String>,
    pub assigned_to_user_id: Option<Uuid>,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub cost: Option<Decimal>,
    pub currency: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateMaintenanceRequest {
    pub title: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub description: Option<Option<String>>,
    pub maintenance_type: Option<String>,
    pub priority: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub supplier_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub rma_reference: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub assigned_to_user_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub scheduled_at: Option<Option<DateTime<Utc>>>,
    #[serde(default, deserialize_with = "double_option")]
    pub cost: Option<Option<Decimal>>,
    pub currency: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CompleteMaintenanceRequest {
    pub completion_notes: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub cost: Option<Option<Decimal>>,
    /// Optional override; defaults to the asset status captured when the ticket opened.
    pub restore_status_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct CancelMaintenanceRequest {
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MaintenanceResponse {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub asset_tag: String,
    pub asset_name: String,
    pub title: String,
    pub description: Option<String>,
    pub maintenance_type: String,
    pub status: String,
    pub priority: String,
    pub supplier_id: Option<Uuid>,
    pub supplier_name: Option<String>,
    pub rma_reference: Option<String>,
    pub assigned_to_user_id: Option<Uuid>,
    pub assigned_to_user_name: Option<String>,
    /// Status the asset had before the ticket opened; restored on close.
    pub previous_status_id: Option<Uuid>,
    pub previous_status_name: Option<String>,
    pub reported_at: DateTime<Utc>,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub completion_notes: Option<String>,
    pub cost: Option<Decimal>,
    pub currency: String,
    pub created_by_user_id: Option<Uuid>,
    pub created_by_user_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub type MaintenanceListResponse = Vec<MaintenanceResponse>;

// ------------------------------------------------ schedule-driven workflow ----

#[derive(Debug, Deserialize)]
pub struct WorkOrderListQuery {
    pub status: Option<String>,
    pub asset_id: Option<Uuid>,
    pub assigned_to: Option<Uuid>,
    #[serde(rename = "type")]
    pub work_order_type: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkOrderRequest {
    pub asset_id: Uuid,
    #[serde(rename = "type")]
    pub work_order_type: String,
    pub notes: Option<String>,
    pub due_date: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateWorkOrderRequest {
    #[serde(default, deserialize_with = "double_option")]
    pub assigned_to: Option<Option<Uuid>>,
    pub status: Option<String>,
    pub cost: Option<Decimal>,
    #[serde(default, deserialize_with = "double_option")]
    pub notes: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
pub struct AddWorkOrderEventRequest {
    pub note: Option<String>,
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ReportBreakdownRequest {
    pub notes: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AssetRef {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct WorkOrderBoardItem {
    pub id: Uuid,
    pub asset: AssetRef,
    #[serde(rename = "type")]
    pub work_order_type: String,
    pub status: String,
    pub assigned_to: Option<Uuid>,
    pub due_date: Option<DateTime<Utc>>,
    pub cost: Option<Decimal>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct WorkOrderEventResponse {
    pub id: Uuid,
    pub event_type: String,
    pub actor_id: Option<Uuid>,
    pub actor_name: Option<String>,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct WorkOrderDetailResponse {
    pub id: Uuid,
    pub schedule_id: Option<Uuid>,
    pub asset: AssetRef,
    #[serde(rename = "type")]
    pub work_order_type: String,
    pub status: String,
    pub assigned_to: Option<Uuid>,
    pub assigned_to_name: Option<String>,
    pub due_date: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub cost: Option<Decimal>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub events: Vec<WorkOrderEventResponse>,
}

#[derive(Debug, Deserialize)]
pub struct CreateMaintenanceScheduleRequest {
    pub frequency_type: String,
    pub interval: i32,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateMaintenanceScheduleRequest {
    pub interval: Option<i32>,
    pub is_active: Option<bool>,
    pub next_due_at: Option<DateTime<Utc>>,
    pub next_due_operating_hours: Option<Decimal>,
    #[serde(default, deserialize_with = "double_option")]
    pub notes: Option<Option<String>>,
}

#[derive(Debug, Serialize)]
pub struct MaintenanceScheduleResponse {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub frequency_type: String,
    pub interval: i32,
    pub next_due_at: Option<DateTime<Utc>>,
    pub next_due_operating_hours: Option<Decimal>,
    pub is_active: bool,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct MaintenanceHistoryQuery {
    /// Return items strictly older than this timestamp (RFC 3339).
    pub cursor: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct MaintenanceHistoryItem {
    pub occurred_at: DateTime<Utc>,
    pub kind: String,
    pub work_order_id: Option<Uuid>,
    pub schedule_id: Option<Uuid>,
    pub summary: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Default, Serialize)]
pub struct SchedulerRunSummary {
    pub generated: usize,
    pub skipped: usize,
    pub errors: usize,
}
