// Executive dashboard aggregation shapes (all counts/sums computed in SQL).

use rust_decimal::Decimal;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct StatusCount {
    pub status_id: Uuid,
    pub status: String,
    pub code: String,
    pub color_code: Option<String>,
    pub status_type: String,
    pub total: i64,
}

#[derive(Debug, Serialize)]
pub struct ExpiryBuckets {
    pub expired: i64,
    pub within_30_days: i64,
    pub within_60_days: i64,
    pub within_90_days: i64,
}

#[derive(Debug, Serialize)]
pub struct ExpirationsResponse {
    pub warranties: ExpiryBuckets,
    pub licenses: ExpiryBuckets,
}

#[derive(Debug, Serialize)]
pub struct LowStockItem {
    /// `consumable` or `accessory`.
    pub kind: String,
    pub id: Uuid,
    pub name: String,
    pub on_hand: i32,
    pub threshold: i32,
    pub shortfall: i32,
}

#[derive(Debug, Serialize)]
pub struct ValueGroup {
    pub id: Option<Uuid>,
    pub name: String,
    pub currency: String,
    pub assets: i64,
    pub total_value: Decimal,
}

#[derive(Debug, Serialize)]
pub struct CurrencyTotal {
    pub currency: String,
    pub assets: i64,
    pub total_value: Decimal,
}

#[derive(Debug, Serialize)]
pub struct FleetValueResponse {
    pub by_manufacturer: Vec<ValueGroup>,
    pub by_department: Vec<ValueGroup>,
    pub totals: Vec<CurrencyTotal>,
}
