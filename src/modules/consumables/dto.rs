// Request/response shapes for consumables and the stock ledger.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::modules::common::double_option;
use crate::modules::i18n::Translations;

#[derive(Debug, Deserialize)]
pub struct CreateConsumableRequest {
    pub translations: Translations,
    pub category_id: Option<Uuid>,
    pub manufacturer_id: Option<Uuid>,
    pub supplier_id: Option<Uuid>,
    pub location_id: Option<Uuid>,
    pub model_number: Option<String>,
    /// Opening stock; recorded as a `restock` transaction.
    #[serde(default)]
    pub quantity_on_hand: i32,
    #[serde(default)]
    pub min_stock_alert: i32,
    pub purchase_cost: Option<Decimal>,
    pub currency: Option<String>,
    pub order_number: Option<String>,
    pub notes: Option<String>,
    pub image_url: Option<String>,
    #[serde(default)]
    pub is_archived: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateConsumableRequest {
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
    /// Stock is only changed through `POST /consumables/{id}/transactions`.
    pub min_stock_alert: Option<i32>,
    #[serde(default, deserialize_with = "double_option")]
    pub purchase_cost: Option<Option<Decimal>>,
    pub currency: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub order_number: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub notes: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub image_url: Option<Option<String>>,
    pub is_archived: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ConsumableResponse {
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
    pub quantity_on_hand: i32,
    pub min_stock_alert: i32,
    pub is_low_stock: bool,
    pub purchase_cost: Option<Decimal>,
    pub currency: String,
    pub order_number: Option<String>,
    pub notes: Option<String>,
    pub image_url: Option<String>,
    pub is_archived: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct ConsumableListQuery {
    pub q: Option<String>,
    pub category_id: Option<Uuid>,
    /// `true` returns only rows at or below `min_stock_alert`.
    pub low_stock: Option<bool>,
    pub is_archived: Option<bool>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct CreateConsumableTransactionRequest {
    /// `restock`, `issue` or `adjustment`.
    pub transaction_type: String,
    /// Positive for restock/issue; signed delta for adjustment.
    pub quantity: i32,
    /// Who consumed it (issue). Optional for internal write-offs.
    pub user_id: Option<Uuid>,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ConsumableTransactionResponse {
    pub id: Uuid,
    pub consumable_id: Uuid,
    pub consumable_name: String,
    pub transaction_type: String,
    pub quantity: i32,
    pub user_id: Option<Uuid>,
    pub user_name: Option<String>,
    pub notes: Option<String>,
    pub performed_by_user_id: Option<Uuid>,
    pub performed_by_user_name: Option<String>,
    pub created_at: DateTime<Utc>,
}
