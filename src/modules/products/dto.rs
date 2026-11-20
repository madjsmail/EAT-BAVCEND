// Request/response shapes for the product catalog.

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::modules::common::double_option;
use crate::modules::i18n::Translations;

#[derive(Debug, Deserialize)]
pub struct CreateProductRequest {
    /// Derived from the English name when omitted.
    pub code: Option<String>,
    pub translations: Translations,
    pub category_id: Uuid,
    pub manufacturer_id: Option<Uuid>,
    pub depreciation_model_id: Option<Uuid>,
    pub model_code: Option<String>,
    pub eol_type: Option<String>,
    pub eol_date: Option<NaiveDate>,
    pub default_purchase_cost: Option<Decimal>,
    pub default_currency: Option<String>,
    pub default_supplier_id: Option<Uuid>,
    pub min_quantity: Option<i32>,
    pub chip: Option<String>,
    pub operating_system: Option<String>,
    pub notes: Option<String>,
    #[serde(default)]
    pub custom_fields_json: Option<Value>,
    #[serde(default)]
    pub specs_json: Option<Value>,
    #[serde(default)]
    pub is_archived: Option<bool>,
    pub image_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProductRequest {
    pub code: Option<String>,
    pub translations: Option<Translations>,
    pub category_id: Option<Uuid>,
    #[serde(default, deserialize_with = "double_option")]
    pub manufacturer_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub depreciation_model_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub model_code: Option<Option<String>>,
    pub eol_type: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub eol_date: Option<Option<NaiveDate>>,
    #[serde(default, deserialize_with = "double_option")]
    pub default_purchase_cost: Option<Option<Decimal>>,
    pub default_currency: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub default_supplier_id: Option<Option<Uuid>>,
    pub min_quantity: Option<i32>,
    #[serde(default, deserialize_with = "double_option")]
    pub chip: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub operating_system: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub notes: Option<Option<String>>,
    pub custom_fields_json: Option<Value>,
    pub specs_json: Option<Value>,
    pub is_archived: Option<bool>,
    #[serde(default, deserialize_with = "double_option")]
    pub image_url: Option<Option<String>>,
}

#[derive(Debug, Serialize)]
pub struct ProductResponse {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub translations: Translations,
    pub category_id: Uuid,
    pub category_name: Option<String>,
    pub manufacturer_id: Option<Uuid>,
    pub manufacturer_name: Option<String>,
    pub depreciation_model_id: Option<Uuid>,
    pub depreciation_model_name: Option<String>,
    pub model_code: Option<String>,
    pub eol_type: String,
    pub eol_date: Option<NaiveDate>,
    pub default_purchase_cost: Option<Decimal>,
    pub default_currency: String,
    pub default_supplier_id: Option<Uuid>,
    pub default_supplier_name: Option<String>,
    pub min_quantity: i32,
    pub chip: Option<String>,
    pub operating_system: Option<String>,
    pub notes: Option<String>,
    pub custom_fields_json: Value,
    pub specs_json: Value,
    pub is_archived: bool,
    pub image_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct ProductListQuery {
    pub q: Option<String>,
    pub category_id: Option<Uuid>,
    pub manufacturer_id: Option<Uuid>,
    pub is_archived: Option<bool>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}
