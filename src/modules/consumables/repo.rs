// Consumable stock ledger. `quantity_on_hand` only moves through
// `consumable_transactions`, and every write locks the consumable row so
// concurrent issues cannot oversell stock.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::api_error::ApiError;
use crate::modules::common::ensure_org_ref;
use crate::modules::consumables::dto::*;
use crate::modules::i18n::Translations;
use crate::repository::audit;

type Result<T> = std::result::Result<T, ApiError>;

fn trimmed(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

async fn replace_translations(
    conn: &mut sqlx::PgConnection,
    consumable_id: Uuid,
    translations: &Translations,
) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM consumable_translations WHERE consumable_id = $1")
        .bind(consumable_id)
        .execute(&mut *conn)
        .await?;
    for (locale, name) in translations {
        sqlx::query(
            "INSERT INTO consumable_translations (consumable_id, locale, name) VALUES ($1, $2, $3)",
        )
        .bind(consumable_id)
        .bind(locale)
        .bind(name.trim())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

#[derive(Debug, FromRow)]
pub struct ConsumableRow {
    pub id: Uuid,
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
    #[sqlx(json)]
    pub translations: std::collections::BTreeMap<String, String>,
}

/// `$1` organization, `$2` locale.
const CONSUMABLE_SELECT: &str = r#"
    SELECT c.id, c.category_id,
           COALESCE((SELECT t.name FROM category_translations t
                     WHERE t.category_id = c.category_id AND t.locale = $2),
                    (SELECT t.name FROM category_translations t
                     WHERE t.category_id = c.category_id AND t.locale = 'en')) AS category_name,
           c.manufacturer_id,
           COALESCE((SELECT t.name FROM manufacturer_translations t
                     WHERE t.manufacturer_id = c.manufacturer_id AND t.locale = $2),
                    (SELECT t.name FROM manufacturer_translations t
                     WHERE t.manufacturer_id = c.manufacturer_id AND t.locale = 'en')) AS manufacturer_name,
           c.supplier_id,
           COALESCE((SELECT t.name FROM supplier_translations t
                     WHERE t.supplier_id = c.supplier_id AND t.locale = $2),
                    (SELECT t.name FROM supplier_translations t
                     WHERE t.supplier_id = c.supplier_id AND t.locale = 'en')) AS supplier_name,
           c.location_id,
           COALESCE((SELECT t.name FROM location_translations t
                     WHERE t.location_id = c.location_id AND t.locale = $2),
                    (SELECT t.name FROM location_translations t
                     WHERE t.location_id = c.location_id AND t.locale = 'en')) AS location_name,
           c.model_number, c.quantity_on_hand, c.min_stock_alert,
           (c.quantity_on_hand <= c.min_stock_alert) AS is_low_stock,
           c.purchase_cost, c.currency, c.order_number, c.notes, c.image_url, c.is_archived,
           c.created_at, c.updated_at,
           COALESCE((SELECT jsonb_object_agg(t.locale, t.name)
                     FROM consumable_translations t WHERE t.consumable_id = c.id), '{}'::jsonb) AS translations
    FROM consumables c
    WHERE c.organization_id = $1
"#;

/// `$3` q, `$4` category, `$5` low_stock, `$6` archived.
const CONSUMABLE_FILTER: &str = r#"
      AND ($3::text IS NULL OR c.model_number ILIKE '%' || $3 || '%'
           OR EXISTS (SELECT 1 FROM consumable_translations s
                      WHERE s.consumable_id = c.id AND s.name ILIKE '%' || $3 || '%'))
      AND ($4::uuid IS NULL OR c.category_id = $4)
      AND ($5::boolean IS NULL
           OR ($5::boolean = TRUE AND c.quantity_on_hand <= c.min_stock_alert)
           OR ($5::boolean = FALSE AND c.quantity_on_hand > c.min_stock_alert))
      AND ($6::boolean IS NULL OR c.is_archived = $6)
"#;

pub async fn list_consumables(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    query: &ConsumableListQuery,
    page: u32,
    per_page: u32,
) -> Result<(Vec<ConsumableRow>, i64)> {
    let sql = format!(
        "{CONSUMABLE_SELECT}{CONSUMABLE_FILTER} ORDER BY c.created_at DESC, c.id LIMIT $7 OFFSET $8"
    );
    let rows = sqlx::query_as::<_, ConsumableRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(query.q.as_deref())
        .bind(query.category_id)
        .bind(query.low_stock)
        .bind(query.is_archived)
        .bind(per_page as i64)
        .bind(((page.saturating_sub(1)) * per_page) as i64)
        .fetch_all(pool)
        .await?;

    let count_sql = format!(
        "SELECT count(*) FROM consumables c WHERE c.organization_id = $1{CONSUMABLE_FILTER}"
    );
    let total: i64 = sqlx::query_scalar(&count_sql)
        .bind(org)
        .bind(locale)
        .bind(query.q.as_deref())
        .bind(query.category_id)
        .bind(query.low_stock)
        .bind(query.is_archived)
        .fetch_one(pool)
        .await?;
    Ok((rows, total))
}

/// Dedicated low-stock feed: archived rows excluded.
pub async fn low_stock_alerts(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
) -> Result<Vec<ConsumableRow>> {
    let sql = format!(
        "{CONSUMABLE_SELECT} AND c.is_archived = FALSE
         AND c.quantity_on_hand <= c.min_stock_alert
         ORDER BY (c.min_stock_alert - c.quantity_on_hand) DESC, c.id"
    );
    Ok(sqlx::query_as::<_, ConsumableRow>(&sql)
        .bind(org)
        .bind(locale)
        .fetch_all(pool)
        .await?)
}

pub async fn get_consumable(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    id: Uuid,
) -> Result<ConsumableRow> {
    let sql = format!("{CONSUMABLE_SELECT} AND c.id = $3");
    sqlx::query_as::<_, ConsumableRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("Consumable"))
}

async fn check_refs(
    conn: &mut sqlx::PgConnection,
    org: Uuid,
    category_id: Option<Uuid>,
    manufacturer_id: Option<Uuid>,
    supplier_id: Option<Uuid>,
    location_id: Option<Uuid>,
) -> Result<()> {
    if let Some(id) = category_id {
        ensure_org_ref(conn, "categories", org, id, "category_id").await?;
    }
    if let Some(id) = manufacturer_id {
        ensure_org_ref(conn, "manufacturers", org, id, "manufacturer_id").await?;
    }
    if let Some(id) = supplier_id {
        ensure_org_ref(conn, "suppliers", org, id, "supplier_id").await?;
    }
    if let Some(id) = location_id {
        ensure_org_ref(conn, "locations", org, id, "location_id").await?;
    }
    Ok(())
}

/// Opening stock is recorded as a ledger row, never as a silent column write.
pub async fn create_consumable(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: &Option<String>,
    req: &CreateConsumableRequest,
) -> Result<ConsumableRow> {
    let mut tx = pool.begin().await?;
    check_refs(
        &mut tx,
        org,
        req.category_id,
        req.manufacturer_id,
        req.supplier_id,
        req.location_id,
    )
    .await?;

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO consumables
            (organization_id, category_id, manufacturer_id, supplier_id, location_id,
             model_number, quantity_on_hand, min_stock_alert, purchase_cost, currency,
             order_number, notes, image_url, is_archived)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, COALESCE($10, 'USD'), $11, $12, $13, $14)
         RETURNING id",
    )
    .bind(org)
    .bind(req.category_id)
    .bind(req.manufacturer_id)
    .bind(req.supplier_id)
    .bind(req.location_id)
    .bind(trimmed(&req.model_number))
    .bind(req.quantity_on_hand)
    .bind(req.min_stock_alert)
    .bind(req.purchase_cost)
    .bind(trimmed(&req.currency).map(|c| c.to_uppercase()))
    .bind(trimmed(&req.order_number))
    .bind(trimmed(&req.notes))
    .bind(trimmed(&req.image_url))
    .bind(req.is_archived)
    .fetch_one(&mut *tx)
    .await?;

    replace_translations(&mut tx, id, &req.translations).await?;

    if req.quantity_on_hand > 0 {
        sqlx::query(
            "INSERT INTO consumable_transactions
                (organization_id, consumable_id, transaction_type, quantity, notes, performed_by_user_id)
             VALUES ($1, $2, 'restock', $3, 'Opening stock', $4)",
        )
        .bind(org)
        .bind(id)
        .bind(req.quantity_on_hand)
        .bind(actor)
        .execute(&mut *tx)
        .await?;
    }

    audit::insert_tx(
        &mut tx,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: Some(actor),
            action: "consumable.create",
            entity_type: "consumable",
            entity_id: Some(id),
            metadata: serde_json::json!({ "quantity_on_hand": req.quantity_on_hand }),
            ip_address: ip.as_deref(),
        },
    )
    .await?;

    tx.commit().await?;
    get_consumable(pool, org, locale, id).await
}

pub async fn update_consumable(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: &Option<String>,
    id: Uuid,
    req: &UpdateConsumableRequest,
) -> Result<ConsumableRow> {
    let current = get_consumable(pool, org, locale, id).await?;
    let mut tx = pool.begin().await?;

    if let Some(Some(category_id)) = req.category_id {
        check_refs(&mut tx, org, Some(category_id), None, None, None).await?;
    }
    if let Some(Some(manufacturer_id)) = req.manufacturer_id {
        check_refs(&mut tx, org, None, Some(manufacturer_id), None, None).await?;
    }
    if let Some(Some(supplier_id)) = req.supplier_id {
        check_refs(&mut tx, org, None, None, Some(supplier_id), None).await?;
    }
    if let Some(Some(location_id)) = req.location_id {
        check_refs(&mut tx, org, None, None, None, Some(location_id)).await?;
    }

    if let Some(translations) = &req.translations {
        replace_translations(&mut tx, id, translations).await?;
    }

    sqlx::query(
        "UPDATE consumables
         SET category_id = $3, manufacturer_id = $4, supplier_id = $5, location_id = $6,
             model_number = $7, min_stock_alert = $8, purchase_cost = $9, currency = $10,
             order_number = $11, notes = $12, image_url = $13, is_archived = $14, updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(id)
    .bind(org)
    .bind(req.category_id.unwrap_or(current.category_id))
    .bind(req.manufacturer_id.unwrap_or(current.manufacturer_id))
    .bind(req.supplier_id.unwrap_or(current.supplier_id))
    .bind(req.location_id.unwrap_or(current.location_id))
    .bind(req.model_number.clone().unwrap_or(current.model_number))
    .bind(req.min_stock_alert.unwrap_or(current.min_stock_alert))
    .bind(req.purchase_cost.unwrap_or(current.purchase_cost))
    .bind(
        req.currency
            .as_deref()
            .map(|c| c.trim().to_uppercase())
            .unwrap_or(current.currency),
    )
    .bind(req.order_number.clone().unwrap_or(current.order_number))
    .bind(req.notes.clone().unwrap_or(current.notes))
    .bind(req.image_url.clone().unwrap_or(current.image_url))
    .bind(req.is_archived.unwrap_or(current.is_archived))
    .execute(&mut *tx)
    .await?;

    audit::insert_tx(
        &mut tx,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: Some(actor),
            action: "consumable.update",
            entity_type: "consumable",
            entity_id: Some(id),
            metadata: serde_json::json!({}),
            ip_address: ip.as_deref(),
        },
    )
    .await?;

    tx.commit().await?;
    get_consumable(pool, org, locale, id).await
}

pub async fn delete_consumable(
    pool: &PgPool,
    org: Uuid,
    actor: Uuid,
    ip: &Option<String>,
    id: Uuid,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let done = sqlx::query("DELETE FROM consumables WHERE id = $1 AND organization_id = $2")
        .bind(id)
        .bind(org)
        .execute(&mut *tx)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound("Consumable"));
    }
    audit::insert_tx(
        &mut tx,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: Some(actor),
            action: "consumable.delete",
            entity_type: "consumable",
            entity_id: Some(id),
            metadata: serde_json::json!({}),
            ip_address: ip.as_deref(),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

// ------------------------------------------------------------- ledger -------

#[derive(Debug, FromRow)]
pub struct TransactionRow {
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

/// `$1` organization, `$2` locale.
const TRANSACTION_SELECT: &str = r#"
    SELECT t.id, t.consumable_id,
           COALESCE((SELECT n.name FROM consumable_translations n
                     WHERE n.consumable_id = t.consumable_id AND n.locale = $2),
                    (SELECT n.name FROM consumable_translations n
                     WHERE n.consumable_id = t.consumable_id AND n.locale = 'en')) AS consumable_name,
           t.transaction_type, t.quantity, t.user_id,
           (SELECT NULLIF(btrim(u.first_name || ' ' || u.last_name), '')
              FROM users u WHERE u.id = t.user_id) AS user_name,
           t.notes, t.performed_by_user_id,
           (SELECT NULLIF(btrim(u.first_name || ' ' || u.last_name), '')
              FROM users u WHERE u.id = t.performed_by_user_id) AS performed_by_user_name,
           t.created_at
    FROM consumable_transactions t
    WHERE t.organization_id = $1
"#;

pub async fn list_transactions(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    consumable_id: Uuid,
) -> Result<Vec<TransactionRow>> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM consumables WHERE id = $1 AND organization_id = $2)",
    )
    .bind(consumable_id)
    .bind(org)
    .fetch_one(pool)
    .await?;
    if !exists {
        return Err(ApiError::NotFound("Consumable"));
    }
    let sql =
        format!("{TRANSACTION_SELECT} AND t.consumable_id = $3 ORDER BY t.created_at DESC, t.id");
    Ok(sqlx::query_as::<_, TransactionRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(consumable_id)
        .fetch_all(pool)
        .await?)
}

pub async fn get_transaction(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    consumable_id: Uuid,
    transaction_id: Uuid,
) -> Result<TransactionRow> {
    let sql = format!("{TRANSACTION_SELECT} AND t.consumable_id = $3 AND t.id = $4");
    sqlx::query_as::<_, TransactionRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(consumable_id)
        .bind(transaction_id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("Consumable transaction"))
}

/// Apply a ledger entry atomically. The row lock serialises concurrent issues.
pub async fn create_transaction(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: &Option<String>,
    consumable_id: Uuid,
    req: &CreateConsumableTransactionRequest,
) -> Result<TransactionRow> {
    let delta = match req.transaction_type.as_str() {
        "restock" => req.quantity,
        "issue" => -req.quantity,
        "adjustment" => req.quantity,
        other => {
            return Err(ApiError::bad_request(
                "transaction_type",
                "invalid",
                format!("unsupported transaction_type: {other}"),
            ));
        }
    };

    let mut tx = pool.begin().await?;

    let current: Option<i32> = sqlx::query_scalar(
        "SELECT quantity_on_hand FROM consumables
         WHERE id = $1 AND organization_id = $2 FOR UPDATE",
    )
    .bind(consumable_id)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    let current = current.ok_or(ApiError::NotFound("Consumable"))?;

    let next = current + delta;
    if next < 0 {
        return Err(ApiError::Conflict(format!(
            "Only {current} unit(s) on hand"
        )));
    }

    if let Some(user_id) = req.user_id {
        ensure_org_ref(&mut tx, "users", org, user_id, "user_id").await?;
    }

    sqlx::query("UPDATE consumables SET quantity_on_hand = $3, updated_at = now() WHERE id = $1 AND organization_id = $2")
        .bind(consumable_id)
        .bind(org)
        .bind(next)
        .execute(&mut *tx)
        .await?;

    let transaction_id: Uuid = sqlx::query_scalar(
        "INSERT INTO consumable_transactions
            (organization_id, consumable_id, transaction_type, quantity, user_id, notes,
             performed_by_user_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(org)
    .bind(consumable_id)
    .bind(&req.transaction_type)
    .bind(req.quantity)
    .bind(req.user_id)
    .bind(trimmed(&req.notes))
    .bind(actor)
    .fetch_one(&mut *tx)
    .await?;

    let action = match req.transaction_type.as_str() {
        "restock" => "consumable.restock",
        "issue" => "consumable.issue",
        _ => "consumable.adjustment",
    };

    audit::insert_tx(
        &mut tx,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: Some(actor),
            action,
            entity_type: "consumable",
            entity_id: Some(consumable_id),
            metadata: serde_json::json!({
                "transaction_id": transaction_id,
                "quantity": req.quantity,
                "delta": delta,
                "quantity_on_hand": next,
                "user_id": req.user_id,
            }),
            ip_address: ip.as_deref(),
        },
    )
    .await?;

    tx.commit().await?;
    get_transaction(pool, org, locale, consumable_id, transaction_id).await
}

/// Consumables issued to a user (used by the 360 view).
pub async fn list_issued_to_user(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    user_id: Uuid,
) -> Result<Vec<TransactionRow>> {
    let sql = format!(
        "{TRANSACTION_SELECT} AND t.user_id = $3 AND t.transaction_type = 'issue'
         ORDER BY t.created_at DESC, t.id"
    );
    Ok(sqlx::query_as::<_, TransactionRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(user_id)
        .fetch_all(pool)
        .await?)
}
