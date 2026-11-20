// Request/response shapes for accessories and their bulk assignments.

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::modules::common::double_option;
use crate::modules::i18n::Translations;

#[derive(Debug, Deserialize)]
pub struct CreateAccessoryRequest {
    pub translations: Translations,
    pub category_id: Option<Uuid>,
    pub manufacturer_id: Option<Uuid>,
    pub supplier_id: Option<Uuid>,
    pub location_id: Option<Uuid>,
    pub model_number: Option<String>,
    #[serde(default)]
    pub total_quantity: i32,
    #[serde(default)]
    pub min_quantity: i32,
    pub purchase_cost: Option<Decimal>,
    pub currency: Option<String>,
    pub purchase_date: Option<NaiveDate>,
    pub order_number: Option<String>,
    pub notes: Option<String>,
    pub image_url: Option<String>,
    #[serde(default)]
    pub is_archived: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAccessoryRequest {
    pub translations: Option<Translations>,
    #[serde(default, deserialize_with = "double_option")]
    pub category_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub manufacturer_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub supplier_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub location_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub model_number: Option<Option<String>>,
    pub total_quantity: Option<i32>,
    pub min_quantity: Option<i32>,
    #[serde(default, deserialize_with = "double_option")]
    pub purchase_cost: Option<Option<Decimal>>,
    pub currency: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub purchase_date: Option<Option<NaiveDate>>,
    #[serde(default, deserialize_with = "double_option")]
    pub order_number: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub notes: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub image_url: Option<Option<String>>,
    pub is_archived: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct AccessoryResponse {
    pub id: Uuid,
    pub name: String,
    pub translations: Translations,
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    pub manufacturer_id: Option<Uuid>,
    pub manufacturer_name: Option<String>,
    pub supplier_id: Option<Uuid>,
    pub supplier_name: Option<String>,
    pub location_id: Option<Uuid>,
    pub location_name: Option<String>,
    pub model_number: Option<String>,
    pub total_quantity: i32,
    pub available_quantity: i32,
    pub assigned_quantity: i32,
    pub min_quantity: i32,
    pub purchase_cost: Option<Decimal>,
    pub currency: String,
    pub purchase_date: Option<NaiveDate>,
    pub order_number: Option<String>,
    pub notes: Option<String>,
    pub image_url: Option<String>,
    pub is_archived: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct AccessoryListQuery {
    pub q: Option<String>,
    pub category_id: Option<Uuid>,
    pub manufacturer_id: Option<Uuid>,
    pub is_archived: Option<bool>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct AssignAccessoryRequest {
    /// Assign to an employee...
    pub user_id: Option<Uuid>,
    /// ...or pair with a specific asset (charger with a laptop/phone).
    pub asset_id: Option<Uuid>,
    pub quantity: i32,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReturnAccessoryRequest {
    /// Defaults to the whole outstanding quantity.
    pub quantity: Option<i32>,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AccessoryAssignmentResponse {
    pub id: Uuid,
    pub accessory_id: Uuid,
    pub accessory_name: String,
    pub assigned_to_user_id: Option<Uuid>,
    pub assigned_to_user_name: Option<String>,
    pub asset_id: Option<Uuid>,
    pub asset_tag: Option<String>,
    pub quantity: i32,
    pub returned_quantity: i32,
    pub remaining_quantity: i32,
    pub notes: Option<String>,
    pub assigned_by_user_id: Option<Uuid>,
    pub assigned_by_user_name: Option<String>,
    pub returned_by_user_id: Option<Uuid>,
    pub returned_by_user_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub returned_at: Option<DateTime<Utc>>,
}
