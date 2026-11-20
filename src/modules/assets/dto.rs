// Request/response shapes for assets, custody and labels.

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::modules::common::double_option;

#[derive(Debug, Deserialize)]
pub struct CreateAssetRequest {
    /// Omit to auto-generate `DEPT.CAT.PROD.NNNN`.
    pub asset_tag: Option<String>,
    pub product_id: Uuid,
    /// Defaults to the organization's deployable status.
    pub status_id: Option<Uuid>,
    pub supplier_id: Option<Uuid>,
    pub location_id: Option<Uuid>,
    pub department_id: Uuid,
    pub name: String,
    pub serial_number: Option<String>,
    pub warranty_date: Option<NaiveDate>,
    pub order_number: Option<String>,
    pub purchase_date: Option<NaiveDate>,
    pub purchase_cost: Option<Decimal>,
    pub currency: Option<String>,
    pub schedule_audit: Option<DateTime<Utc>>,
    pub notes: Option<String>,
    pub image_url: Option<String>,
    pub imei_1: Option<String>,
    pub imei_2: Option<String>,
    #[serde(default)]
    pub mdm_enrolled: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAssetRequest {
    pub asset_tag: Option<String>,
    pub product_id: Option<Uuid>,
    pub status_id: Option<Uuid>,
    #[serde(default, deserialize_with = "double_option")]
    pub supplier_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub location_id: Option<Option<Uuid>>,
    pub department_id: Option<Uuid>,
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub serial_number: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub warranty_date: Option<Option<NaiveDate>>,
    #[serde(default, deserialize_with = "double_option")]
    pub order_number: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub purchase_date: Option<Option<NaiveDate>>,
    #[serde(default, deserialize_with = "double_option")]
    pub purchase_cost: Option<Option<Decimal>>,
    pub currency: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub schedule_audit: Option<Option<DateTime<Utc>>>,
    #[serde(default, deserialize_with = "double_option")]
    pub notes: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub image_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub imei_1: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub imei_2: Option<Option<String>>,
    pub mdm_enrolled: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct AssetResponse {
    pub id: Uuid,
    pub asset_tag: String,
    pub name: String,
    pub product_id: Uuid,
    pub product_name: Option<String>,
    pub product_code: Option<String>,
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    pub manufacturer_name: Option<String>,
    pub status_id: Uuid,
    pub status_name: Option<String>,
    pub status_type: Option<String>,
    pub status_color_code: Option<String>,
    pub supplier_id: Option<Uuid>,
    pub supplier_name: Option<String>,
    pub location_id: Option<Uuid>,
    pub location_name: Option<String>,
    pub department_id: Uuid,
    pub department_name: Option<String>,
    pub assigned_user_id: Option<Uuid>,
    pub assigned_user_name: Option<String>,
    pub serial_number: Option<String>,
    pub warranty_date: Option<NaiveDate>,
    pub order_number: Option<String>,
    pub purchase_date: Option<NaiveDate>,
    pub purchase_cost: Option<Decimal>,
    pub currency: String,
    pub schedule_audit: Option<DateTime<Utc>>,
    pub notes: Option<String>,
    pub image_url: Option<String>,
    pub imei_1: Option<String>,
    pub imei_2: Option<String>,
    pub mdm_enrolled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct AssetListQuery {
    pub q: Option<String>,
    pub status_id: Option<Uuid>,
    pub location_id: Option<Uuid>,
    pub department_id: Option<Uuid>,
    pub product_id: Option<Uuid>,
    pub assigned_user_id: Option<Uuid>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

// --------------------------------------------------------------- custody ----

#[derive(Debug, Deserialize)]
pub struct AssignAssetRequest {
    /// Assign to a user; the asset inherits the user's location.
    pub user_id: Option<Uuid>,
    /// Or park it at a location without a user.
    pub location_id: Option<Uuid>,
    pub status_id: Option<Uuid>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReturnAssetRequest {
    /// Optional destination (e.g. a stock location).
    pub location_id: Option<Uuid>,
    pub status_id: Option<Uuid>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TransferAssetRequest {
    pub location_id: Uuid,
    pub department_id: Option<Uuid>,
    pub status_id: Option<Uuid>,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AssignmentResponse {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub action: String,
    pub assigned_to_user_id: Option<Uuid>,
    pub assigned_to_user_name: Option<String>,
    pub from_user_id: Option<Uuid>,
    pub from_user_name: Option<String>,
    pub from_location_id: Option<Uuid>,
    pub from_location_name: Option<String>,
    pub to_location_id: Option<Uuid>,
    pub to_location_name: Option<String>,
    pub from_department_id: Option<Uuid>,
    pub to_department_id: Option<Uuid>,
    pub from_status_id: Option<Uuid>,
    pub to_status_id: Option<Uuid>,
    pub notes: Option<String>,
    pub performed_by_user_id: Option<Uuid>,
    pub performed_by_user_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Unified 360 view for the asset detail page: core record + everything that
/// currently hangs off the asset (accessories, license seats) and off its
/// assigned user (consumables issued, user-held seats).
#[derive(Debug, Serialize)]
pub struct Asset360Response {
    pub asset: AssetResponse,
    pub accessories: Vec<crate::modules::accessories::dto::AccessoryAssignmentResponse>,
    pub consumables: Vec<crate::modules::consumables::dto::ConsumableTransactionResponse>,
    pub licenses: Vec<crate::modules::licenses::dto::LicenseSeatResponse>,
    pub custody_history: Vec<AssignmentResponse>,
    pub counts: Asset360Counts,
}

#[derive(Debug, Serialize)]
pub struct Asset360Counts {
    pub accessories: usize,
    pub consumables: usize,
    pub licenses: usize,
    pub custody_events: usize,
}

/// JSON label payload; `svg`/`png` variants return rendered image bytes.
#[derive(Debug, Serialize)]
pub struct LabelPayload {
    pub asset_id: Uuid,
    pub asset_tag: String,
    pub serial_number: Option<String>,
    pub name: String,
    pub status_name: Option<String>,
    pub location_name: Option<String>,
    pub department_name: Option<String>,
    pub assigned_user_name: Option<String>,
    pub data: String,
    pub data_source: String,
    pub symbology: String,
}
