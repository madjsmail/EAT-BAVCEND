// Asset registry and custody data access.
//
// `assets` holds the current holder/status/location; `asset_assignments` is an
// append-only history. Every mutation also writes an `audit_logs` row inside
// the same transaction.

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::api_error::ApiError;
use crate::modules::assets::dto::*;
use crate::modules::common::{ensure_org_ref, normalize_code};
use crate::modules::lookups::repo as lookups;
use crate::repository::audit;

type Result<T> = std::result::Result<T, ApiError>;

fn trimmed(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

#[derive(Debug, FromRow)]
pub struct AssetRow {
    pub id: Uuid,
    pub asset_tag: String,
    pub name: String,
    pub product_id: Uuid,
    pub product_code: String,
    pub product_name: Option<String>,
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

/// `$1` organization, `$2` locale (localized names fall back to English).
const ASSET_SELECT: &str = r#"
    SELECT a.id, a.asset_tag, a.name, a.product_id, p.code AS product_code,
           COALESCE((SELECT t.name FROM product_translations t
                     WHERE t.product_id = a.product_id AND t.locale = $2),
                    (SELECT t.name FROM product_translations t
                     WHERE t.product_id = a.product_id AND t.locale = 'en')) AS product_name,
           p.category_id,
           COALESCE((SELECT t.name FROM category_translations t
                     WHERE t.category_id = p.category_id AND t.locale = $2),
                    (SELECT t.name FROM category_translations t
                     WHERE t.category_id = p.category_id AND t.locale = 'en')) AS category_name,
           COALESCE((SELECT t.name FROM manufacturer_translations t
                     WHERE t.manufacturer_id = p.manufacturer_id AND t.locale = $2),
                    (SELECT t.name FROM manufacturer_translations t
                     WHERE t.manufacturer_id = p.manufacturer_id AND t.locale = 'en')) AS manufacturer_name,
           a.status_id,
           COALESCE((SELECT t.name FROM status_translations t
                     WHERE t.status_id = a.status_id AND t.locale = $2),
                    (SELECT t.name FROM status_translations t
                     WHERE t.status_id = a.status_id AND t.locale = 'en')) AS status_name,
           s.status_type, s.color_code AS status_color_code,
           a.supplier_id,
           COALESCE((SELECT t.name FROM supplier_translations t
                     WHERE t.supplier_id = a.supplier_id AND t.locale = $2),
                    (SELECT t.name FROM supplier_translations t
                     WHERE t.supplier_id = a.supplier_id AND t.locale = 'en')) AS supplier_name,
           a.location_id,
           COALESCE((SELECT t.name FROM location_translations t
                     WHERE t.location_id = a.location_id AND t.locale = $2),
                    (SELECT t.name FROM location_translations t
                     WHERE t.location_id = a.location_id AND t.locale = 'en')) AS location_name,
           a.department_id,
           COALESCE((SELECT t.name FROM department_translations t
                     WHERE t.department_id = a.department_id AND t.locale = $2),
                    (SELECT t.name FROM department_translations t
                     WHERE t.department_id = a.department_id AND t.locale = 'en')) AS department_name,
           a.assigned_user_id,
           (SELECT NULLIF(btrim(u.first_name || ' ' || u.last_name), '')
              FROM users u WHERE u.id = a.assigned_user_id) AS assigned_user_name,
           a.serial_number, a.warranty_date, a.order_number, a.purchase_date, a.purchase_cost,
           a.currency, a.schedule_audit, a.notes, a.image_url, a.imei_1, a.imei_2,
           a.mdm_enrolled, a.created_at, a.updated_at
    FROM assets a
    JOIN products p ON p.id = a.product_id
    LEFT JOIN statuses s ON s.id = a.status_id
    WHERE a.organization_id = $1
"#;

const ASSET_FILTER: &str = r#"
      AND ($3::text IS NULL OR a.asset_tag ILIKE '%' || $3 || '%'
           OR a.serial_number ILIKE '%' || $3 || '%'
           OR a.name ILIKE '%' || $3 || '%'
           OR a.imei_1 ILIKE '%' || $3 || '%'
           OR a.imei_2 ILIKE '%' || $3 || '%'
           OR p.code ILIKE '%' || $3 || '%')
      AND ($4::uuid IS NULL OR a.status_id = $4)
      AND ($5::uuid IS NULL OR a.location_id = $5)
      AND ($6::uuid IS NULL OR a.department_id = $6)
      AND ($7::uuid IS NULL OR a.product_id = $7)
      AND ($8::uuid IS NULL OR a.assigned_user_id = $8)
"#;

pub async fn list_assets(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    query: &AssetListQuery,
    page: u32,
    per_page: u32,
) -> Result<(Vec<AssetRow>, i64)> {
    let sql = format!(
        "{ASSET_SELECT}{ASSET_FILTER} ORDER BY a.created_at DESC, a.id LIMIT $9 OFFSET $10"
    );
    let rows = sqlx::query_as::<_, AssetRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(query.q.as_deref())
        .bind(query.status_id)
        .bind(query.location_id)
        .bind(query.department_id)
        .bind(query.product_id)
        .bind(query.assigned_user_id)
        .bind(per_page as i64)
        .bind(((page.saturating_sub(1)) * per_page) as i64)
        .fetch_all(pool)
        .await?;

    let count_sql = format!(
        "SELECT count(*) FROM assets a JOIN products p ON p.id = a.product_id
         WHERE a.organization_id = $1{ASSET_FILTER}"
    );
    let total: i64 = sqlx::query_scalar(&count_sql)
        .bind(org)
        .bind(locale)
        .bind(query.q.as_deref())
        .bind(query.status_id)
        .bind(query.location_id)
        .bind(query.department_id)
        .bind(query.product_id)
        .bind(query.assigned_user_id)
        .fetch_one(pool)
        .await?;

    Ok((rows, total))
}

pub async fn get_asset(pool: &PgPool, org: Uuid, locale: &str, id: Uuid) -> Result<AssetRow> {
    let sql = format!("{ASSET_SELECT} AND a.id = $3");
    sqlx::query_as::<_, AssetRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("Asset"))
}

/// Quick-scan: exact match on tag or serial, then a loose fallback.
pub async fn scan_asset(pool: &PgPool, org: Uuid, locale: &str, code: &str) -> Result<AssetRow> {
    let term = code.trim();
    let sql = format!(
        "{ASSET_SELECT}
         AND (a.asset_tag = $3 OR lower(a.serial_number) = lower($3)
              OR a.imei_1 = $3 OR a.imei_2 = $3)"
    );
    if let Some(row) = sqlx::query_as::<_, AssetRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(term)
        .fetch_optional(pool)
        .await?
    {
        return Ok(row);
    }

    let loose = format!("{ASSET_SELECT} AND a.asset_tag ILIKE '%' || $3 || '%' LIMIT 1");
    sqlx::query_as::<_, AssetRow>(&loose)
        .bind(org)
        .bind(locale)
        .bind(term)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("Asset"))
}

/// `DEPT.CAT.PROD.NNNN`, sequential per (department, product).
async fn next_asset_tag(
    conn: &mut sqlx::PgConnection,
    org: Uuid,
    department_id: Uuid,
    product_id: Uuid,
) -> Result<String> {
    let department_code: String =
        sqlx::query_scalar("SELECT code FROM departments WHERE id = $1 AND organization_id = $2")
            .bind(department_id)
            .bind(org)
            .fetch_optional(&mut *conn)
            .await?
            .ok_or_else(|| {
                ApiError::bad_request(
                    "department_id",
                    "not_found",
                    "department_id does not exist in this organization",
                )
            })?;

    let (product_code, category_code) =
        crate::modules::products::repo::product_tag_codes(conn, org, product_id).await?;

    let sequence: i32 = sqlx::query_scalar(
        "INSERT INTO asset_sequences (organization_id, department_id, product_id, last_value)
         VALUES ($1, $2, $3, 1)
         ON CONFLICT (organization_id, department_id, product_id)
         DO UPDATE SET last_value = asset_sequences.last_value + 1
         RETURNING last_value",
    )
    .bind(org)
    .bind(department_id)
    .bind(product_id)
    .fetch_one(&mut *conn)
    .await?;

    Ok(format!(
        "{department_code}.{category_code}.{product_code}.{sequence:04}"
    ))
}

pub async fn create_asset(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: Option<String>,
    req: &CreateAssetRequest,
) -> Result<AssetRow> {
    let mut tx = pool.begin().await?;

    ensure_org_ref(&mut tx, "products", org, req.product_id, "product_id").await?;
    ensure_org_ref(
        &mut tx,
        "departments",
        org,
        req.department_id,
        "department_id",
    )
    .await?;
    if let Some(id) = req.supplier_id {
        ensure_org_ref(&mut tx, "suppliers", org, id, "supplier_id").await?;
    }
    if let Some(id) = req.location_id {
        ensure_org_ref(&mut tx, "locations", org, id, "location_id").await?;
    }

    let status_id = match req.status_id {
        Some(id) => {
            ensure_org_ref(&mut tx, "statuses", org, id, "status_id").await?;
            id
        }
        None => lookups::find_status_for_transition(&mut tx, org, "ready_to_deploy", "deployable")
            .await?
            .ok_or_else(|| {
                ApiError::bad_request(
                    "status_id",
                    "required",
                    "No deployable status is configured; pass status_id",
                )
            })?,
    };

    let asset_tag = match trimmed(&req.asset_tag) {
        Some(tag) => normalize_code(&tag),
        None => next_asset_tag(&mut tx, org, req.department_id, req.product_id).await?,
    };

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO assets
            (organization_id, asset_tag, product_id, status_id, supplier_id, location_id,
             department_id, name, serial_number, warranty_date, order_number, purchase_date,
             purchase_cost, currency, schedule_audit, notes, image_url, imei_1, imei_2,
             mdm_enrolled)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                 COALESCE($14, 'USD'), $15, $16, $17, $18, $19, $20)
         RETURNING id",
    )
    .bind(org)
    .bind(&asset_tag)
    .bind(req.product_id)
    .bind(status_id)
    .bind(req.supplier_id)
    .bind(req.location_id)
    .bind(req.department_id)
    .bind(req.name.trim())
    .bind(trimmed(&req.serial_number))
    .bind(req.warranty_date)
    .bind(trimmed(&req.order_number))
    .bind(req.purchase_date)
    .bind(req.purchase_cost)
    .bind(trimmed(&req.currency).map(|c| c.to_uppercase()))
    .bind(req.schedule_audit)
    .bind(trimmed(&req.notes))
    .bind(trimmed(&req.image_url))
    .bind(trimmed(&req.imei_1))
    .bind(trimmed(&req.imei_2))
    .bind(req.mdm_enrolled)
    .fetch_one(&mut *tx)
    .await?;

    audit::insert_tx(
        &mut tx,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: Some(actor),
            action: "asset.create",
            entity_type: "asset",
            entity_id: Some(id),
            metadata: serde_json::json!({
                "asset_tag": asset_tag,
                "product_id": req.product_id,
                "department_id": req.department_id,
                "status_id": status_id,
            }),
            ip_address: ip.as_deref(),
        },
    )
    .await?;

    tx.commit().await?;
    get_asset(pool, org, locale, id).await
}

pub async fn update_asset(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: Option<String>,
    id: Uuid,
    req: &UpdateAssetRequest,
) -> Result<AssetRow> {
    let current = get_asset(pool, org, locale, id).await?;
    let mut tx = pool.begin().await?;

    if let Some(product_id) = req.product_id {
        ensure_org_ref(&mut tx, "products", org, product_id, "product_id").await?;
    }
    if let Some(department_id) = req.department_id {
        ensure_org_ref(&mut tx, "departments", org, department_id, "department_id").await?;
    }
    if let Some(status_id) = req.status_id {
        ensure_org_ref(&mut tx, "statuses", org, status_id, "status_id").await?;
    }
    if let Some(Some(supplier_id)) = req.supplier_id {
        ensure_org_ref(&mut tx, "suppliers", org, supplier_id, "supplier_id").await?;
    }
    if let Some(Some(location_id)) = req.location_id {
        ensure_org_ref(&mut tx, "locations", org, location_id, "location_id").await?;
    }

    sqlx::query(
        "UPDATE assets
         SET asset_tag = $3, product_id = $4, status_id = $5, supplier_id = $6,
             location_id = $7, department_id = $8, name = $9, serial_number = $10,
             warranty_date = $11, order_number = $12, purchase_date = $13, purchase_cost = $14,
             currency = $15, schedule_audit = $16, notes = $17, image_url = $18, imei_1 = $19,
             imei_2 = $20, mdm_enrolled = $21, updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(id)
    .bind(org)
    .bind(
        req.asset_tag
            .as_deref()
            .map(normalize_code)
            .unwrap_or_else(|| current.asset_tag.clone()),
    )
    .bind(req.product_id.unwrap_or(current.product_id))
    .bind(req.status_id.unwrap_or(current.status_id))
    .bind(req.supplier_id.unwrap_or(current.supplier_id))
    .bind(req.location_id.unwrap_or(current.location_id))
    .bind(req.department_id.unwrap_or(current.department_id))
    .bind(req.name.as_deref().map(str::trim).unwrap_or(&current.name))
    .bind(req.serial_number.clone().unwrap_or(current.serial_number))
    .bind(req.warranty_date.unwrap_or(current.warranty_date))
    .bind(req.order_number.clone().unwrap_or(current.order_number))
    .bind(req.purchase_date.unwrap_or(current.purchase_date))
    .bind(req.purchase_cost.unwrap_or(current.purchase_cost))
    .bind(
        req.currency
            .as_deref()
            .map(|c| c.trim().to_uppercase())
            .unwrap_or(current.currency),
    )
    .bind(req.schedule_audit.unwrap_or(current.schedule_audit))
    .bind(req.notes.clone().unwrap_or(current.notes))
    .bind(req.image_url.clone().unwrap_or(current.image_url))
    .bind(req.imei_1.clone().unwrap_or(current.imei_1))
    .bind(req.imei_2.clone().unwrap_or(current.imei_2))
    .bind(req.mdm_enrolled.unwrap_or(current.mdm_enrolled))
    .execute(&mut *tx)
    .await?;

    audit::insert_tx(
        &mut tx,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: Some(actor),
            action: "asset.update",
            entity_type: "asset",
            entity_id: Some(id),
            metadata: serde_json::json!({ "asset_tag": current.asset_tag }),
            ip_address: ip.as_deref(),
        },
    )
    .await?;

    tx.commit().await?;
    get_asset(pool, org, locale, id).await
}

pub async fn delete_asset(
    pool: &PgPool,
    org: Uuid,
    actor: Uuid,
    ip: Option<String>,
    id: Uuid,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let tag: Option<String> = sqlx::query_scalar(
        "DELETE FROM assets WHERE id = $1 AND organization_id = $2 RETURNING asset_tag",
    )
    .bind(id)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(tag) = tag else {
        return Err(ApiError::NotFound("Asset"));
    };

    audit::insert_tx(
        &mut tx,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: Some(actor),
            action: "asset.delete",
            entity_type: "asset",
            entity_id: Some(id),
            metadata: serde_json::json!({ "asset_tag": tag }),
            ip_address: ip.as_deref(),
        },
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

// --------------------------------------------------------------- custody ----

/// Current custody columns, read under `FOR UPDATE`.
struct CustodySnapshot {
    status_id: Uuid,
    location_id: Option<Uuid>,
    department_id: Uuid,
    assigned_user_id: Option<Uuid>,
}

async fn lock_asset(conn: &mut sqlx::PgConnection, org: Uuid, id: Uuid) -> Result<CustodySnapshot> {
    let row = sqlx::query_as::<_, (Uuid, Option<Uuid>, Uuid, Option<Uuid>)>(
        "SELECT status_id, location_id, department_id, assigned_user_id
         FROM assets WHERE id = $1 AND organization_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(org)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(ApiError::NotFound("Asset"))?;

    Ok(CustodySnapshot {
        status_id: row.0,
        location_id: row.1,
        department_id: row.2,
        assigned_user_id: row.3,
    })
}

async fn insert_assignment(
    conn: &mut sqlx::PgConnection,
    org: Uuid,
    asset_id: Uuid,
    action: &str,
    assigned_to_user_id: Option<Uuid>,
    from: &CustodySnapshot,
    to_location_id: Option<Uuid>,
    to_department_id: Option<Uuid>,
    to_status_id: Option<Uuid>,
    notes: Option<&str>,
    performed_by: Uuid,
) -> Result<Uuid> {
    sqlx::query_scalar(
        "INSERT INTO asset_assignments
            (organization_id, asset_id, action, assigned_to_user_id, from_user_id, from_location_id,
             to_location_id, from_department_id, to_department_id, from_status_id, to_status_id,
             notes, performed_by_user_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
         RETURNING id",
    )
    .bind(org)
    .bind(asset_id)
    .bind(action)
    .bind(assigned_to_user_id)
    .bind(from.assigned_user_id)
    .bind(from.location_id)
    .bind(to_location_id)
    .bind(from.department_id)
    .bind(to_department_id)
    .bind(from.status_id)
    .bind(to_status_id)
    .bind(notes)
    .bind(performed_by)
    .fetch_one(&mut *conn)
    .await
    .map_err(ApiError::from)
}

async fn audit_custody(
    conn: &mut sqlx::PgConnection,
    org: Uuid,
    actor: Uuid,
    ip: Option<String>,
    asset_id: Uuid,
    action: &str,
    from: &CustodySnapshot,
    to_status_id: Uuid,
    to_location_id: Option<Uuid>,
    to_department_id: Uuid,
    to_user_id: Option<Uuid>,
) -> sqlx::Result<()> {
    audit::insert_tx(
        conn,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: Some(actor),
            action,
            entity_type: "asset",
            entity_id: Some(asset_id),
            metadata: serde_json::json!({
                "from": {
                    "status_id": from.status_id,
                    "location_id": from.location_id,
                    "department_id": from.department_id,
                    "assigned_user_id": from.assigned_user_id,
                },
                "to": {
                    "status_id": to_status_id,
                    "location_id": to_location_id,
                    "department_id": to_department_id,
                    "assigned_user_id": to_user_id,
                },
            }),
            ip_address: ip.as_deref(),
        },
    )
    .await
}

/// Assign to a user (asset inherits the user's location) or park at a location.
pub async fn assign_asset(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: Option<String>,
    asset_id: Uuid,
    req: &AssignAssetRequest,
) -> Result<AssetRow> {
    if req.user_id.is_some() == req.location_id.is_some() {
        return Err(ApiError::bad_request(
            "user_id",
            "invalid",
            "Provide exactly one of user_id or location_id",
        ));
    }

    let mut tx = pool.begin().await?;
    let from = lock_asset(&mut tx, org, asset_id).await?;

    let (to_user_id, to_location_id) = match req.user_id {
        Some(user_id) => {
            let location_id: Option<Option<Uuid>> = sqlx::query_scalar(
                "SELECT location_id FROM users WHERE id = $1 AND organization_id = $2",
            )
            .bind(user_id)
            .bind(org)
            .fetch_optional(&mut *tx)
            .await?;
            let Some(location_id) = location_id else {
                return Err(ApiError::bad_request(
                    "user_id",
                    "not_found",
                    "user_id does not exist in this organization",
                ));
            };
            let Some(location_id) = location_id else {
                return Err(ApiError::bad_request(
                    "user_id",
                    "location_required",
                    "This user has no location; set users.location_id before assigning assets",
                ));
            };
            (Some(user_id), Some(location_id))
        }
        None => {
            let location_id = req.location_id.unwrap_or_default();
            ensure_org_ref(&mut tx, "locations", org, location_id, "location_id").await?;
            (None, Some(location_id))
        }
    };

    let status_id = match req.status_id {
        Some(id) => {
            ensure_org_ref(&mut tx, "statuses", org, id, "status_id").await?;
            id
        }
        None => lookups::find_status_for_transition(&mut tx, org, "deployed", "deployed")
            .await?
            .ok_or_else(|| {
                ApiError::bad_request(
                    "status_id",
                    "required",
                    "No deployed status is configured; pass status_id",
                )
            })?,
    };

    sqlx::query(
        "UPDATE assets
         SET assigned_user_id = $3, location_id = $4, status_id = $5, updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(asset_id)
    .bind(org)
    .bind(to_user_id)
    .bind(to_location_id)
    .bind(status_id)
    .execute(&mut *tx)
    .await?;

    insert_assignment(
        &mut tx,
        org,
        asset_id,
        "assign",
        to_user_id,
        &from,
        to_location_id,
        Some(from.department_id),
        Some(status_id),
        req.notes.as_deref(),
        actor,
    )
    .await?;

    audit_custody(
        &mut tx,
        org,
        actor,
        ip,
        asset_id,
        "asset.assign",
        &from,
        status_id,
        to_location_id,
        from.department_id,
        to_user_id,
    )
    .await?;

    tx.commit().await?;
    get_asset(pool, org, locale, asset_id).await
}

/// Return to stock: clears the holder, moves to a deployable status.
pub async fn return_asset(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: Option<String>,
    asset_id: Uuid,
    req: &ReturnAssetRequest,
) -> Result<AssetRow> {
    let mut tx = pool.begin().await?;
    let from = lock_asset(&mut tx, org, asset_id).await?;

    if let Some(location_id) = req.location_id {
        ensure_org_ref(&mut tx, "locations", org, location_id, "location_id").await?;
    }
    let to_location_id = req.location_id.or(from.location_id);

    let status_id = match req.status_id {
        Some(id) => {
            ensure_org_ref(&mut tx, "statuses", org, id, "status_id").await?;
            id
        }
        None => lookups::find_status_for_transition(&mut tx, org, "ready_to_deploy", "deployable")
            .await?
            .ok_or_else(|| {
                ApiError::bad_request(
                    "status_id",
                    "required",
                    "No deployable status is configured; pass status_id",
                )
            })?,
    };

    sqlx::query(
        "UPDATE assets
         SET assigned_user_id = NULL, location_id = $3, status_id = $4, updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(asset_id)
    .bind(org)
    .bind(to_location_id)
    .bind(status_id)
    .execute(&mut *tx)
    .await?;

    insert_assignment(
        &mut tx,
        org,
        asset_id,
        "return",
        None,
        &from,
        to_location_id,
        Some(from.department_id),
        Some(status_id),
        req.notes.as_deref(),
        actor,
    )
    .await?;

    audit_custody(
        &mut tx,
        org,
        actor,
        ip,
        asset_id,
        "asset.return",
        &from,
        status_id,
        to_location_id,
        from.department_id,
        None,
    )
    .await?;

    tx.commit().await?;
    get_asset(pool, org, locale, asset_id).await
}

/// Move an asset between locations (and optionally departments).
pub async fn transfer_asset(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: Option<String>,
    asset_id: Uuid,
    req: &TransferAssetRequest,
) -> Result<AssetRow> {
    let mut tx = pool.begin().await?;
    let from = lock_asset(&mut tx, org, asset_id).await?;

    ensure_org_ref(&mut tx, "locations", org, req.location_id, "location_id").await?;
    if let Some(department_id) = req.department_id {
        ensure_org_ref(&mut tx, "departments", org, department_id, "department_id").await?;
    }
    let to_department_id = req.department_id.unwrap_or(from.department_id);
    let status_id = match req.status_id {
        Some(id) => {
            ensure_org_ref(&mut tx, "statuses", org, id, "status_id").await?;
            id
        }
        None => from.status_id,
    };

    sqlx::query(
        "UPDATE assets
         SET location_id = $3, department_id = $4, status_id = $5, updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(asset_id)
    .bind(org)
    .bind(req.location_id)
    .bind(to_department_id)
    .bind(status_id)
    .execute(&mut *tx)
    .await?;

    insert_assignment(
        &mut tx,
        org,
        asset_id,
        "transfer",
        from.assigned_user_id,
        &from,
        Some(req.location_id),
        Some(to_department_id),
        Some(status_id),
        req.notes.as_deref(),
        actor,
    )
    .await?;

    audit_custody(
        &mut tx,
        org,
        actor,
        ip,
        asset_id,
        "asset.transfer",
        &from,
        status_id,
        Some(req.location_id),
        to_department_id,
        from.assigned_user_id,
    )
    .await?;

    tx.commit().await?;
    get_asset(pool, org, locale, asset_id).await
}

#[derive(Debug, FromRow)]
pub struct AssignmentRow {
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

pub async fn list_assignments(
    pool: &PgPool,
    org: Uuid,
    asset_id: Uuid,
) -> Result<Vec<AssignmentRow>> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM assets WHERE id = $1 AND organization_id = $2)",
    )
    .bind(asset_id)
    .bind(org)
    .fetch_one(pool)
    .await?;
    if !exists {
        return Err(ApiError::NotFound("Asset"));
    }

    Ok(sqlx::query_as::<_, AssignmentRow>(
        r#"
        SELECT h.id, h.asset_id, h.action, h.assigned_to_user_id,
               (SELECT NULLIF(btrim(u.first_name || ' ' || u.last_name), '')
                  FROM users u WHERE u.id = h.assigned_to_user_id) AS assigned_to_user_name,
               h.from_user_id,
               (SELECT NULLIF(btrim(u.first_name || ' ' || u.last_name), '')
                  FROM users u WHERE u.id = h.from_user_id) AS from_user_name,
               h.from_location_id,
               COALESCE((SELECT t.name FROM location_translations t
                         WHERE t.location_id = h.from_location_id AND t.locale = 'en')) AS from_location_name,
               h.to_location_id,
               COALESCE((SELECT t.name FROM location_translations t
                         WHERE t.location_id = h.to_location_id AND t.locale = 'en')) AS to_location_name,
               h.from_department_id, h.to_department_id, h.from_status_id, h.to_status_id,
               h.notes, h.performed_by_user_id,
               (SELECT NULLIF(btrim(u.first_name || ' ' || u.last_name), '')
                  FROM users u WHERE u.id = h.performed_by_user_id) AS performed_by_user_name,
               h.created_at
        FROM asset_assignments h
        WHERE h.organization_id = $1 AND h.asset_id = $2
        ORDER BY h.created_at DESC, h.id
        "#,
    )
    .bind(org)
    .bind(asset_id)
    .fetch_all(pool)
    .await?)
}
