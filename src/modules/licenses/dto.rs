// Request/response shapes for software licenses and seat allocation.

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::modules::common::double_option;
use crate::modules::i18n::Translations;

#[derive(Debug, Deserialize)]
pub struct CreateLicenseRequest {
    pub translations: Translations,
    pub manufacturer_id: Option<Uuid>,
    pub supplier_id: Option<Uuid>,
    pub license_type: Option<String>,
    #[serde(default)]
    pub total_seats: i32,
    pub purchase_date: Option<NaiveDate>,
    pub expiration_date: Option<NaiveDate>,
    pub purchase_cost: Option<Decimal>,
    pub currency: Option<String>,
    pub order_number: Option<String>,
    pub product_key: Option<String>,
    pub notes: Option<String>,
    #[serde(default)]
    pub is_archived: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateLicenseRequest {
    pub translations: Option<Translations>,
    #[serde(default, deserialize_with = "double_option")]
    pub manufacturer_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub supplier_id: Option<Option<Uuid>>,
    pub license_type: Option<String>,
    pub total_seats: Option<i32>,
    #[serde(default, deserialize_with = "double_option")]
    pub purchase_date: Option<Option<NaiveDate>>,
    #[serde(default, deserialize_with = "double_option")]
    pub expiration_date: Option<Option<NaiveDate>>,
    #[serde(default, deserialize_with = "double_option")]
    pub purchase_cost: Option<Option<Decimal>>,
    pub currency: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub order_number: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub product_key: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub notes: Option<Option<String>>,
    pub is_archived: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct LicenseResponse {
    pub id: Uuid,
    pub name: String,
    pub translations: Translations,
    pub manufacturer_id: Option<Uuid>,
    pub manufacturer_name: Option<String>,
    pub supplier_id: Option<Uuid>,
    pub supplier_name: Option<String>,
    pub license_type: String,
    pub total_seats: i32,
    pub active_seats: i32,
    pub available_seats: i32,
    pub purchase_date: Option<NaiveDate>,
    pub expiration_date: Option<NaiveDate>,
    /// Negative when expired.
    pub days_until_expiry: Option<i32>,
    /// `expired`, `30`, `60`, `90` or null when outside every window.
    pub expiry_bucket: Option<String>,
    pub purchase_cost: Option<Decimal>,
    pub currency: String,
    pub order_number: Option<String>,
    pub product_key: Option<String>,
    pub notes: Option<String>,
    pub is_archived: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct LicenseListQuery {
    pub q: Option<String>,
    /// Only licenses expiring within the next N days (not expired).
    pub expiring_in_days: Option<i32>,
    pub is_archived: Option<bool>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ExpiryAlerts {
    pub expired: Vec<LicenseResponse>,
    pub within_30_days: Vec<LicenseResponse>,
    pub within_60_days: Vec<LicenseResponse>,
    pub within_90_days: Vec<LicenseResponse>,
}

#[derive(Debug, Deserialize)]
pub struct AllocateSeatRequest {
    /// Allocate to an employee...
    pub user_id: Option<Uuid>,
    /// ...or to a specific asset.
    pub asset_id: Option<Uuid>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct RevokeSeatRequest {
    pub notes: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LicenseSeatResponse {
    pub id: Uuid,
    pub license_id: Uuid,
    pub license_name: String,
    pub user_id: Option<Uuid>,
    pub user_name: Option<String>,
    pub asset_id: Option<Uuid>,
    pub asset_tag: Option<String>,
    pub notes: Option<String>,
    pub assigned_by_user_id: Option<Uuid>,
    pub assigned_by_user_name: Option<String>,
    pub revoked_by_user_id: Option<Uuid>,
    pub revoked_by_user_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}
