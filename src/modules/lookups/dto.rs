// Request/response shapes for the lookup CRUD endpoints.
//
// Every localized lookup takes `translations` as a locale -> name map. PATCH
// bodies are partial: an absent field is unchanged, `null` clears a nullable
// column, and a `translations` object replaces the whole map.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::modules::common::double_option;
use crate::modules::i18n::Translations;

// ------------------------------------------------------------ categories ----

#[derive(Debug, Deserialize)]
pub struct CreateCategoryRequest {
    pub code: Option<String>,
    pub translations: Translations,
    pub category_type: String,
    #[serde(default)]
    pub skip_checkout_confirmation_emails: bool,
    pub icon_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCategoryRequest {
    pub code: Option<String>,
    pub translations: Option<Translations>,
    pub category_type: Option<String>,
    pub skip_checkout_confirmation_emails: Option<bool>,
    #[serde(default, deserialize_with = "double_option")]
    pub icon_url: Option<Option<String>>,
}

#[derive(Debug, Serialize)]
pub struct CategoryResponse {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub translations: Translations,
    pub category_type: String,
    pub skip_checkout_confirmation_emails: bool,
    pub icon_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// --------------------------------------------------------- manufacturers ----

#[derive(Debug, Deserialize)]
pub struct CreateManufacturerRequest {
    pub translations: Translations,
    pub url: Option<String>,
    pub support_url: Option<String>,
    pub support_phone: Option<String>,
    pub support_email: Option<String>,
    pub notes: Option<String>,
    pub logo_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateManufacturerRequest {
    pub translations: Option<Translations>,
    #[serde(default, deserialize_with = "double_option")]
    pub url: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub support_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub support_phone: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub support_email: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub notes: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub logo_url: Option<Option<String>>,
}

#[derive(Debug, Serialize)]
pub struct ManufacturerResponse {
    pub id: Uuid,
    pub name: String,
    pub translations: Translations,
    pub url: Option<String>,
    pub support_url: Option<String>,
    pub support_phone: Option<String>,
    pub support_email: Option<String>,
    pub notes: Option<String>,
    pub logo_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// --------------------------------------------------------------- statuses ----

#[derive(Debug, Deserialize)]
pub struct CreateStatusRequest {
    /// Stable machine key; derived from the English name when omitted.
    pub code: Option<String>,
    pub translations: Translations,
    pub status_type: String,
    pub color_code: Option<String>,
    #[serde(default)]
    pub show_in_nav: bool,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateStatusRequest {
    pub translations: Option<Translations>,
    pub status_type: Option<String>,
    pub color_code: Option<String>,
    pub show_in_nav: Option<bool>,
    #[serde(default, deserialize_with = "double_option")]
    pub notes: Option<Option<String>>,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub translations: Translations,
    pub status_type: String,
    pub color_code: String,
    pub show_in_nav: bool,
    pub notes: Option<String>,
    pub assets_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ------------------------------------------------------------ departments ----

#[derive(Debug, Deserialize)]
pub struct CreateDepartmentRequest {
    pub code: Option<String>,
    pub translations: Translations,
    pub email: Option<String>,
    pub phone_number: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateDepartmentRequest {
    pub code: Option<String>,
    pub translations: Option<Translations>,
    #[serde(default, deserialize_with = "double_option")]
    pub email: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub phone_number: Option<Option<String>>,
}

#[derive(Debug, Serialize)]
pub struct DepartmentResponse {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub translations: Translations,
    pub email: Option<String>,
    pub phone_number: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// -------------------------------------------------------------- locations ----

#[derive(Debug, Deserialize)]
pub struct CreateLocationRequest {
    pub code: Option<String>,
    pub translations: Translations,
    pub parent_id: Option<Uuid>,
    pub location_type: Option<String>,
    pub address_line1: Option<String>,
    pub address_line2: Option<String>,
    pub city: Option<String>,
    pub state_region: Option<String>,
    pub postal_code: Option<String>,
    pub country_code: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub phone_number: Option<String>,
    pub email: Option<String>,
    pub manager_user_id: Option<Uuid>,
    pub notes: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateLocationRequest {
    pub code: Option<String>,
    pub translations: Option<Translations>,
    #[serde(default, deserialize_with = "double_option")]
    pub parent_id: Option<Option<Uuid>>,
    pub location_type: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub address_line1: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub address_line2: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub city: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub state_region: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub postal_code: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub country_code: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub latitude: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub longitude: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub phone_number: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub email: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub manager_user_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub notes: Option<Option<String>>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct LocationResponse {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub translations: Translations,
    pub parent_id: Option<Uuid>,
    pub location_type: String,
    pub address_line1: Option<String>,
    pub address_line2: Option<String>,
    pub city: Option<String>,
    pub state_region: Option<String>,
    pub postal_code: Option<String>,
    pub country_code: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub phone_number: Option<String>,
    pub email: Option<String>,
    pub manager_user_id: Option<Uuid>,
    pub notes: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// -------------------------------------------------------------- suppliers ----

#[derive(Debug, Deserialize)]
pub struct CreateSupplierRequest {
    pub translations: Translations,
    pub contact_name: Option<String>,
    pub email: Option<String>,
    pub phone_number: Option<String>,
    pub address: Option<String>,
    pub url: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSupplierRequest {
    pub translations: Option<Translations>,
    #[serde(default, deserialize_with = "double_option")]
    pub contact_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub email: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub phone_number: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub address: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub url: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub notes: Option<Option<String>>,
}

#[derive(Debug, Serialize)]
pub struct SupplierResponse {
    pub id: Uuid,
    pub name: String,
    pub translations: Translations,
    pub contact_name: Option<String>,
    pub email: Option<String>,
    pub phone_number: Option<String>,
    pub address: Option<String>,
    pub url: Option<String>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// --------------------------------------------------- depreciation models ----

#[derive(Debug, Deserialize)]
pub struct CreateDepreciationModelRequest {
    pub translations: Translations,
    pub months: i32,
    pub floor_value: Option<Decimal>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateDepreciationModelRequest {
    pub translations: Option<Translations>,
    pub months: Option<i32>,
    pub floor_value: Option<Decimal>,
    #[serde(default, deserialize_with = "double_option")]
    pub notes: Option<Option<String>>,
}

#[derive(Debug, Serialize)]
pub struct DepreciationModelResponse {
    pub id: Uuid,
    pub name: String,
    pub translations: Translations,
    pub months: i32,
    pub floor_value: Decimal,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
