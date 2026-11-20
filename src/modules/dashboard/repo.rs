// Dashboard aggregations. Every number is produced by a GROUP BY / FILTER
// aggregate so the API never filters rows in memory.

use rust_decimal::Decimal;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::api_error::ApiError;
use crate::modules::dashboard::dto::*;

type Result<T> = std::result::Result<T, ApiError>;

pub async fn asset_status(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
) -> Result<Vec<StatusCount>> {
    #[derive(FromRow)]
    struct Row {
        status_id: Uuid,
        status: Option<String>,
        code: String,
        color_code: Option<String>,
        status_type: String,
        total: i64,
    }

    let rows = sqlx::query_as::<_, Row>(
        r#"
        SELECT s.id AS status_id,
               COALESCE((SELECT t.name FROM status_translations t
                         WHERE t.status_id = s.id AND t.locale = $2),
                        (SELECT t.name FROM status_translations t
                         WHERE t.status_id = s.id AND t.locale = 'en')) AS status,
               s.code, s.color_code, s.status_type,
               count(a.id) AS total
        FROM statuses s
        LEFT JOIN assets a ON a.status_id = s.id AND a.organization_id = $1
        WHERE s.organization_id = $1
        GROUP BY s.id, s.code, s.color_code, s.status_type
        ORDER BY count(a.id) DESC, s.code
        "#,
    )
    .bind(org)
    .bind(locale)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| StatusCount {
            status_id: r.status_id,
            status: r.status.unwrap_or_else(|| r.code.clone()),
            code: r.code,
            color_code: r.color_code,
            status_type: r.status_type,
            total: r.total,
        })
        .collect())
}

#[derive(FromRow, Default)]
struct BucketRow {
    expired: i64,
    within_30_days: i64,
    within_60_days: i64,
    within_90_days: i64,
}

impl From<BucketRow> for ExpiryBuckets {
    fn from(r: BucketRow) -> Self {
        Self {
            expired: r.expired,
            within_30_days: r.within_30_days,
            within_60_days: r.within_60_days,
            within_90_days: r.within_90_days,
        }
    }
}

pub async fn expirations(pool: &PgPool, org: Uuid) -> Result<ExpirationsResponse> {
    let warranties: BucketRow = sqlx::query_as(
        r#"
        SELECT count(*) FILTER (WHERE warranty_date < current_date) AS expired,
               count(*) FILTER (WHERE warranty_date >= current_date
                                  AND warranty_date <= current_date + 30) AS within_30_days,
               count(*) FILTER (WHERE warranty_date > current_date + 30
                                  AND warranty_date <= current_date + 60) AS within_60_days,
               count(*) FILTER (WHERE warranty_date > current_date + 60
                                  AND warranty_date <= current_date + 90) AS within_90_days
        FROM assets
        WHERE organization_id = $1 AND warranty_date IS NOT NULL
        "#,
    )
    .bind(org)
    .fetch_one(pool)
    .await?;

    let licenses: BucketRow = sqlx::query_as(
        r#"
        SELECT count(*) FILTER (WHERE expiration_date < current_date) AS expired,
               count(*) FILTER (WHERE expiration_date >= current_date
                                  AND expiration_date <= current_date + 30) AS within_30_days,
               count(*) FILTER (WHERE expiration_date > current_date + 30
                                  AND expiration_date <= current_date + 60) AS within_60_days,
               count(*) FILTER (WHERE expiration_date > current_date + 60
                                  AND expiration_date <= current_date + 90) AS within_90_days
        FROM licenses
        WHERE organization_id = $1 AND expiration_date IS NOT NULL AND is_archived = FALSE
        "#,
    )
    .bind(org)
    .fetch_one(pool)
    .await?;

    Ok(ExpirationsResponse {
        warranties: warranties.into(),
        licenses: licenses.into(),
    })
}

#[derive(FromRow)]
struct LowStockRow {
    kind: String,
    id: Uuid,
    name: Option<String>,
    on_hand: i32,
    threshold: i32,
    shortfall: i32,
}

pub async fn low_stock(pool: &PgPool, org: Uuid, locale: &str) -> Result<Vec<LowStockItem>> {
    let rows = sqlx::query_as::<_, LowStockRow>(
        r#"
        SELECT 'consumable' AS kind, c.id,
               COALESCE((SELECT t.name FROM consumable_translations t
                         WHERE t.consumable_id = c.id AND t.locale = $2),
                        (SELECT t.name FROM consumable_translations t
                         WHERE t.consumable_id = c.id AND t.locale = 'en')) AS name,
               c.quantity_on_hand AS on_hand, c.min_stock_alert AS threshold,
               c.min_stock_alert - c.quantity_on_hand AS shortfall
        FROM consumables c
        WHERE c.organization_id = $1 AND c.is_archived = FALSE
          AND c.quantity_on_hand <= c.min_stock_alert
        UNION ALL
        SELECT 'accessory' AS kind, a.id,
               COALESCE((SELECT t.name FROM accessory_translations t
                         WHERE t.accessory_id = a.id AND t.locale = $2),
                        (SELECT t.name FROM accessory_translations t
                         WHERE t.accessory_id = a.id AND t.locale = 'en')) AS name,
               a.available_quantity AS on_hand, a.min_quantity AS threshold,
               a.min_quantity - a.available_quantity AS shortfall
        FROM accessories a
        WHERE a.organization_id = $1 AND a.is_archived = FALSE
          AND a.available_quantity <= a.min_quantity
        ORDER BY shortfall DESC, kind, id
        "#,
    )
    .bind(org)
    .bind(locale)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| LowStockItem {
            kind: r.kind,
            id: r.id,
            name: r.name.unwrap_or_default(),
            on_hand: r.on_hand,
            threshold: r.threshold,
            shortfall: r.shortfall,
        })
        .collect())
}

#[derive(FromRow)]
struct GroupRow {
    id: Option<Uuid>,
    name: Option<String>,
    currency: String,
    assets: i64,
    total_value: Decimal,
}

#[derive(FromRow)]
struct TotalRow {
    currency: String,
    assets: i64,
    total_value: Decimal,
}

impl From<GroupRow> for ValueGroup {
    fn from(r: GroupRow) -> Self {
        Self {
            id: r.id,
            name: r.name.unwrap_or_else(|| "Unassigned".to_string()),
            currency: r.currency,
            assets: r.assets,
            total_value: r.total_value,
        }
    }
}

pub async fn fleet_value(pool: &PgPool, org: Uuid, locale: &str) -> Result<FleetValueResponse> {
    let by_manufacturer = sqlx::query_as::<_, GroupRow>(
        r#"
        SELECT p.manufacturer_id AS id,
               COALESCE((SELECT t.name FROM manufacturer_translations t
                         WHERE t.manufacturer_id = p.manufacturer_id AND t.locale = $2),
                        (SELECT t.name FROM manufacturer_translations t
                         WHERE t.manufacturer_id = p.manufacturer_id AND t.locale = 'en')) AS name,
               a.currency,
               count(*) AS assets,
               COALESCE(sum(a.purchase_cost), 0) AS total_value
        FROM assets a
        JOIN products p ON p.id = a.product_id
        WHERE a.organization_id = $1
        GROUP BY p.manufacturer_id, a.currency
        ORDER BY total_value DESC
        "#,
    )
    .bind(org)
    .bind(locale)
    .fetch_all(pool)
    .await?;

    let by_department = sqlx::query_as::<_, GroupRow>(
        r#"
        SELECT a.department_id AS id,
               COALESCE((SELECT t.name FROM department_translations t
                         WHERE t.department_id = a.department_id AND t.locale = $2),
                        (SELECT t.name FROM department_translations t
                         WHERE t.department_id = a.department_id AND t.locale = 'en')) AS name,
               a.currency,
               count(*) AS assets,
               COALESCE(sum(a.purchase_cost), 0) AS total_value
        FROM assets a
        WHERE a.organization_id = $1
        GROUP BY a.department_id, a.currency
        ORDER BY total_value DESC
        "#,
    )
    .bind(org)
    .bind(locale)
    .fetch_all(pool)
    .await?;

    let totals = sqlx::query_as::<_, TotalRow>(
        r#"
        SELECT currency, count(*) AS assets, COALESCE(sum(purchase_cost), 0) AS total_value
        FROM assets
        WHERE organization_id = $1
        GROUP BY currency
        ORDER BY total_value DESC
        "#,
    )
    .bind(org)
    .fetch_all(pool)
    .await?;

    Ok(FleetValueResponse {
        by_manufacturer: by_manufacturer.into_iter().map(Into::into).collect(),
        by_department: by_department.into_iter().map(Into::into).collect(),
        totals: totals
            .into_iter()
            .map(|t| CurrencyTotal {
                currency: t.currency,
                assets: t.assets,
                total_value: t.total_value,
            })
            .collect(),
    })
}
