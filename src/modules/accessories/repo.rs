// Accessory data access. Stock mutations lock the accessory row, so
// `available_quantity` can never be double-spent.

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::api_error::ApiError;
use crate::modules::accessories::dto::*;
use crate::modules::common::ensure_org_ref;
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
    accessory_id: Uuid,
    translations: &Translations,
) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM accessory_translations WHERE accessory_id = $1")
        .bind(accessory_id)
        .execute(&mut *conn)
        .await?;
    for (locale, name) in translations {
        sqlx::query(
            "INSERT INTO accessory_translations (accessory_id, locale, name) VALUES ($1, $2, $3)",
        )
        .bind(accessory_id)
        .bind(locale)
        .bind(name.trim())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

#[derive(Debug, FromRow)]
pub struct AccessoryRow {
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
    #[sqlx(json)]
    pub translations: std::collections::BTreeMap<String, String>,
}

/// `$1` organization, `$2` locale.
const ACCESSORY_SELECT: &str = r#"
    SELECT ac.id, ac.category_id,
           COALESCE((SELECT t.name FROM category_translations t
                     WHERE t.category_id = ac.category_id AND t.locale = $2),
                    (SELECT t.name FROM category_translations t
                     WHERE t.category_id = ac.category_id AND t.locale = 'en')) AS category_name,
           ac.manufacturer_id,
           COALESCE((SELECT t.name FROM manufacturer_translations t
                     WHERE t.manufacturer_id = ac.manufacturer_id AND t.locale = $2),
                    (SELECT t.name FROM manufacturer_translations t
                     WHERE t.manufacturer_id = ac.manufacturer_id AND t.locale = 'en')) AS manufacturer_name,
           ac.supplier_id,
           COALESCE((SELECT t.name FROM supplier_translations t
                     WHERE t.supplier_id = ac.supplier_id AND t.locale = $2),
                    (SELECT t.name FROM supplier_translations t
                     WHERE t.supplier_id = ac.supplier_id AND t.locale = 'en')) AS supplier_name,
           ac.location_id,
           COALESCE((SELECT t.name FROM location_translations t
                     WHERE t.location_id = ac.location_id AND t.locale = $2),
                    (SELECT t.name FROM location_translations t
                     WHERE t.location_id = ac.location_id AND t.locale = 'en')) AS location_name,
           ac.model_number, ac.total_quantity, ac.available_quantity,
           (ac.total_quantity - ac.available_quantity) AS assigned_quantity,
           ac.min_quantity, ac.purchase_cost, ac.currency, ac.purchase_date, ac.order_number,
           ac.notes, ac.image_url, ac.is_archived, ac.created_at, ac.updated_at,
           COALESCE((SELECT jsonb_object_agg(t.locale, t.name)
                     FROM accessory_translations t WHERE t.accessory_id = ac.id), '{}'::jsonb) AS translations
    FROM accessories ac
    WHERE ac.organization_id = $1
"#;

/// `$3` q, `$4` category, `$5` manufacturer, `$6` archived.
const ACCESSORY_FILTER: &str = r#"
      AND ($3::text IS NULL OR ac.model_number ILIKE '%' || $3 || '%'
           OR EXISTS (SELECT 1 FROM accessory_translations s
                      WHERE s.accessory_id = ac.id AND s.name ILIKE '%' || $3 || '%'))
      AND ($4::uuid IS NULL OR ac.category_id = $4)
      AND ($5::uuid IS NULL OR ac.manufacturer_id = $5)
      AND ($6::boolean IS NULL OR ac.is_archived = $6)
"#;

pub async fn list_accessories(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    query: &AccessoryListQuery,
    page: u32,
    per_page: u32,
) -> Result<(Vec<AccessoryRow>, i64)> {
    let sql = format!(
        "{ACCESSORY_SELECT}{ACCESSORY_FILTER} ORDER BY ac.created_at DESC, ac.id LIMIT $7 OFFSET $8"
    );
    let rows = sqlx::query_as::<_, AccessoryRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(query.q.as_deref())
        .bind(query.category_id)
        .bind(query.manufacturer_id)
        .bind(query.is_archived)
        .bind(per_page as i64)
        .bind(((page.saturating_sub(1)) * per_page) as i64)
        .fetch_all(pool)
        .await?;

    let count_sql = format!(
        "SELECT count(*) FROM accessories ac WHERE ac.organization_id = $1{ACCESSORY_FILTER}"
    );
    let total: i64 = sqlx::query_scalar(&count_sql)
        .bind(org)
        .bind(locale)
        .bind(query.q.as_deref())
        .bind(query.category_id)
        .bind(query.manufacturer_id)
        .bind(query.is_archived)
        .fetch_one(pool)
        .await?;
    Ok((rows, total))
}

pub async fn get_accessory(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    id: Uuid,
) -> Result<AccessoryRow> {
    let sql = format!("{ACCESSORY_SELECT} AND ac.id = $3");
    sqlx::query_as::<_, AccessoryRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("Accessory"))
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

pub async fn create_accessory(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: &Option<String>,
    req: &CreateAccessoryRequest,
) -> Result<AccessoryRow> {
    let mut tx = pool.begin().await?;
    audit::bind_actor(
        &mut tx,
        audit::Actor {
            user_id: Some(actor),
            ip_address: ip.as_deref(),
            user_agent: None,
        },
    )
    .await?;
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
        "INSERT INTO accessories
            (organization_id, category_id, manufacturer_id, supplier_id, location_id,
             model_number, total_quantity, available_quantity, min_quantity, purchase_cost,
             currency, purchase_date, order_number, notes, image_url, is_archived)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $7, $8, $9,
                 COALESCE($10, 'USD'), $11, $12, $13, $14, $15)
         RETURNING id",
    )
    .bind(org)
    .bind(req.category_id)
    .bind(req.manufacturer_id)
    .bind(req.supplier_id)
    .bind(req.location_id)
    .bind(trimmed(&req.model_number))
    .bind(req.total_quantity)
    .bind(req.min_quantity)
    .bind(req.purchase_cost)
    .bind(trimmed(&req.currency).map(|c| c.to_uppercase()))
    .bind(req.purchase_date)
    .bind(trimmed(&req.order_number))
    .bind(trimmed(&req.notes))
    .bind(trimmed(&req.image_url))
    .bind(req.is_archived)
    .fetch_one(&mut *tx)
    .await?;

    replace_translations(&mut tx, id, &req.translations).await?;


    tx.commit().await?;
    get_accessory(pool, org, locale, id).await
}

pub async fn update_accessory(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: &Option<String>,
    id: Uuid,
    req: &UpdateAccessoryRequest,
) -> Result<AccessoryRow> {
    let current = get_accessory(pool, org, locale, id).await?;
    let mut tx = pool.begin().await?;
    audit::bind_actor(
        &mut tx,
        audit::Actor {
            user_id: Some(actor),
            ip_address: ip.as_deref(),
            user_agent: None,
        },
    )
    .await?;

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

    // Growing total stock makes the new units available; shrinking it is only
    // allowed down to what is currently assigned.
    let total_quantity = req.total_quantity.unwrap_or(current.total_quantity);
    let assigned = current.total_quantity - current.available_quantity;
    if total_quantity < assigned {
        return Err(ApiError::bad_request(
            "total_quantity",
            "invalid",
            "total_quantity cannot be lower than the quantity currently assigned",
        ));
    }
    let available_quantity = total_quantity - assigned;

    if let Some(translations) = &req.translations {
        replace_translations(&mut tx, id, translations).await?;
    }

    sqlx::query(
        "UPDATE accessories
         SET category_id = $3, manufacturer_id = $4, supplier_id = $5, location_id = $6,
             model_number = $7, total_quantity = $8, available_quantity = $9, min_quantity = $10,
             purchase_cost = $11, currency = $12, purchase_date = $13, order_number = $14,
             notes = $15, image_url = $16, is_archived = $17, updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(id)
    .bind(org)
    .bind(req.category_id.unwrap_or(current.category_id))
    .bind(req.manufacturer_id.unwrap_or(current.manufacturer_id))
    .bind(req.supplier_id.unwrap_or(current.supplier_id))
    .bind(req.location_id.unwrap_or(current.location_id))
    .bind(req.model_number.clone().unwrap_or(current.model_number))
    .bind(total_quantity)
    .bind(available_quantity)
    .bind(req.min_quantity.unwrap_or(current.min_quantity))
    .bind(req.purchase_cost.unwrap_or(current.purchase_cost))
    .bind(
        req.currency
            .as_deref()
            .map(|c| c.trim().to_uppercase())
            .unwrap_or(current.currency),
    )
    .bind(req.purchase_date.unwrap_or(current.purchase_date))
    .bind(req.order_number.clone().unwrap_or(current.order_number))
    .bind(req.notes.clone().unwrap_or(current.notes))
    .bind(req.image_url.clone().unwrap_or(current.image_url))
    .bind(req.is_archived.unwrap_or(current.is_archived))
    .execute(&mut *tx)
    .await?;


    tx.commit().await?;
    get_accessory(pool, org, locale, id).await
}

pub async fn delete_accessory(
    pool: &PgPool,
    org: Uuid,
    actor: Uuid,
    ip: &Option<String>,
    id: Uuid,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    audit::bind_actor(
        &mut tx,
        audit::Actor {
            user_id: Some(actor),
            ip_address: ip.as_deref(),
            user_agent: None,
        },
    )
    .await?;
    let done = sqlx::query("DELETE FROM accessories WHERE id = $1 AND organization_id = $2")
        .bind(id)
        .bind(org)
        .execute(&mut *tx)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound("Accessory"));
    }
    tx.commit().await?;
    Ok(())
}

// ------------------------------------------------------------ assignments ----

#[derive(Debug, FromRow)]
pub struct AssignmentRow {
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

/// `$1` organization, `$2` locale.
const ASSIGNMENT_SELECT: &str = r#"
    SELECT h.id, h.accessory_id,
           COALESCE((SELECT t.name FROM accessory_translations t
                     WHERE t.accessory_id = h.accessory_id AND t.locale = $2),
                    (SELECT t.name FROM accessory_translations t
                     WHERE t.accessory_id = h.accessory_id AND t.locale = 'en')) AS accessory_name,
           h.assigned_to_user_id,
           (SELECT NULLIF(btrim(u.first_name || ' ' || u.last_name), '')
              FROM users u WHERE u.id = h.assigned_to_user_id) AS assigned_to_user_name,
           h.asset_id,
           (SELECT a.asset_tag FROM assets a WHERE a.id = h.asset_id) AS asset_tag,
           h.quantity, h.returned_quantity,
           (h.quantity - h.returned_quantity) AS remaining_quantity,
           h.notes, h.assigned_by_user_id,
           (SELECT NULLIF(btrim(u.first_name || ' ' || u.last_name), '')
              FROM users u WHERE u.id = h.assigned_by_user_id) AS assigned_by_user_name,
           h.returned_by_user_id,
           (SELECT NULLIF(btrim(u.first_name || ' ' || u.last_name), '')
              FROM users u WHERE u.id = h.returned_by_user_id) AS returned_by_user_name,
           h.created_at, h.returned_at
    FROM accessory_assignments h
    WHERE h.organization_id = $1
"#;

pub async fn get_assignment(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    accessory_id: Uuid,
    assignment_id: Uuid,
) -> Result<AssignmentRow> {
    let sql = format!("{ASSIGNMENT_SELECT} AND h.accessory_id = $3 AND h.id = $4");
    sqlx::query_as::<_, AssignmentRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(accessory_id)
        .bind(assignment_id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("Accessory assignment"))
}

pub async fn list_assignments(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    accessory_id: Uuid,
) -> Result<Vec<AssignmentRow>> {
    let sql =
        format!("{ASSIGNMENT_SELECT} AND h.accessory_id = $3 ORDER BY h.created_at DESC, h.id");
    Ok(sqlx::query_as::<_, AssignmentRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(accessory_id)
        .fetch_all(pool)
        .await?)
}

/// Accessories currently paired with a hardware asset (used by the 360 view).
pub async fn list_active_for_asset(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    asset_id: Uuid,
) -> Result<Vec<AssignmentRow>> {
    let sql = format!(
        "{ASSIGNMENT_SELECT} AND h.asset_id = $3 AND h.returned_quantity < h.quantity
         ORDER BY h.created_at DESC, h.id"
    );
    Ok(sqlx::query_as::<_, AssignmentRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(asset_id)
        .fetch_all(pool)
        .await?)
}

/// Accessories currently held by a user (used by the 360 view).
pub async fn list_active_for_user(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    user_id: Uuid,
) -> Result<Vec<AssignmentRow>> {
    let sql = format!(
        "{ASSIGNMENT_SELECT} AND h.assigned_to_user_id = $3 AND h.returned_quantity < h.quantity
         ORDER BY h.created_at DESC, h.id"
    );
    Ok(sqlx::query_as::<_, AssignmentRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(user_id)
        .fetch_all(pool)
        .await?)
}

/// Deduct stock and record who holds it. `available_quantity` is locked.
pub async fn assign_accessory(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: &Option<String>,
    accessory_id: Uuid,
    req: &AssignAccessoryRequest,
) -> Result<AssignmentRow> {
    if req.user_id.is_some() == req.asset_id.is_some() {
        return Err(ApiError::bad_request(
            "user_id",
            "invalid",
            "Provide exactly one of user_id or asset_id",
        ));
    }
    if req.quantity <= 0 {
        return Err(ApiError::bad_request(
            "quantity",
            "invalid",
            "quantity must be greater than zero",
        ));
    }

    let mut tx = pool.begin().await?;
    audit::bind_actor(
        &mut tx,
        audit::Actor {
            user_id: Some(actor),
            ip_address: ip.as_deref(),
            user_agent: None,
        },
    )
    .await?;

    let available: Option<i32> = sqlx::query_scalar(
        "SELECT available_quantity FROM accessories
         WHERE id = $1 AND organization_id = $2 FOR UPDATE",
    )
    .bind(accessory_id)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    let available = available.ok_or(ApiError::NotFound("Accessory"))?;
    if available < req.quantity {
        return Err(ApiError::Conflict(format!(
            "Only {available} unit(s) available"
        )));
    }

    if let Some(user_id) = req.user_id {
        ensure_org_ref(&mut tx, "users", org, user_id, "user_id").await?;
    }
    if let Some(asset_id) = req.asset_id {
        ensure_org_ref(&mut tx, "assets", org, asset_id, "asset_id").await?;
    }

    sqlx::query(
        "UPDATE accessories SET available_quantity = available_quantity - $3, updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(accessory_id)
    .bind(org)
    .bind(req.quantity)
    .execute(&mut *tx)
    .await?;

    let assignment_id: Uuid = sqlx::query_scalar(
        "INSERT INTO accessory_assignments
            (organization_id, accessory_id, assigned_to_user_id, asset_id, quantity, notes,
             assigned_by_user_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(org)
    .bind(accessory_id)
    .bind(req.user_id)
    .bind(req.asset_id)
    .bind(req.quantity)
    .bind(trimmed(&req.notes))
    .bind(actor)
    .fetch_one(&mut *tx)
    .await?;

    audit::insert_tx(
        &mut tx,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: Some(actor),
            action: "assigned",
            entity_type: "accessory",
            entity_id: Some(accessory_id),
            metadata: serde_json::json!({
                "assignment_id": assignment_id,
                "quantity": req.quantity,
                "user_id": req.user_id,
                "asset_id": req.asset_id,
            }),
            ip_address: ip.as_deref(),
        },
    )
    .await?;

    tx.commit().await?;
    get_assignment(pool, org, locale, accessory_id, assignment_id).await
}

/// Return some or all units; stock is added back atomically.
pub async fn return_accessory(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: &Option<String>,
    accessory_id: Uuid,
    assignment_id: Uuid,
    req: &ReturnAccessoryRequest,
) -> Result<AssignmentRow> {
    let mut tx = pool.begin().await?;
    audit::bind_actor(
        &mut tx,
        audit::Actor {
            user_id: Some(actor),
            ip_address: ip.as_deref(),
            user_agent: None,
        },
    )
    .await?;

    let row: Option<(i32, i32)> = sqlx::query_as(
        "SELECT quantity, returned_quantity FROM accessory_assignments
         WHERE id = $1 AND accessory_id = $2 AND organization_id = $3 FOR UPDATE",
    )
    .bind(assignment_id)
    .bind(accessory_id)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    let (quantity, returned) = row.ok_or(ApiError::NotFound("Accessory assignment"))?;
    let remaining = quantity - returned;
    if remaining <= 0 {
        return Err(ApiError::Conflict(
            "This assignment is already fully returned".to_string(),
        ));
    }

    let to_return = req.quantity.unwrap_or(remaining);
    if to_return <= 0 || to_return > remaining {
        return Err(ApiError::bad_request(
            "quantity",
            "invalid",
            format!("quantity must be between 1 and {remaining}"),
        ));
    }

    sqlx::query(
        "UPDATE accessory_assignments
         SET returned_quantity = returned_quantity + $3,
             returned_by_user_id = $4,
             notes = COALESCE($5, notes),
             returned_at = CASE WHEN returned_quantity + $3 >= quantity THEN now() ELSE returned_at END
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(assignment_id)
    .bind(org)
    .bind(to_return)
    .bind(actor)
    .bind(trimmed(&req.notes))
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "UPDATE accessories SET available_quantity = available_quantity + $3, updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(accessory_id)
    .bind(org)
    .bind(to_return)
    .execute(&mut *tx)
    .await?;

    audit::insert_tx(
        &mut tx,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: Some(actor),
            action: "returned",
            entity_type: "accessory",
            entity_id: Some(accessory_id),
            metadata: serde_json::json!({
                "assignment_id": assignment_id,
                "quantity": to_return,
            }),
            ip_address: ip.as_deref(),
        },
    )
    .await?;

    tx.commit().await?;
    get_assignment(pool, org, locale, accessory_id, assignment_id).await
}
