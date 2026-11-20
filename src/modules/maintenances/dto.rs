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
