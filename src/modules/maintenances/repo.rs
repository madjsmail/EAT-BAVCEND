// Maintenance ticket data access.
//
// Opening a ticket captures the asset's current status in `previous_status_id`
// and parks the asset in `being_repaired`; closing (complete/cancel) restores
// that exact status. All writes run in a transaction with an audit row.

use chrono::{DateTime, Duration, Months, Utc};
use rust_decimal::Decimal;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::api_error::ApiError;
use crate::modules::common::{ensure_org_ref, validate_enum};
use crate::modules::lookups::repo as lookups;
use crate::modules::maintenances::dto::*;
use crate::repository::audit;

type Result<T> = std::result::Result<T, ApiError>;

pub const MAINTENANCE_TYPES: [&str; 3] = ["preventive", "corrective", "inspection"];
pub const MAINTENANCE_STATUSES: [&str; 4] = ["open", "in_progress", "completed", "cancelled"];
pub const MAINTENANCE_PRIORITIES: [&str; 4] = ["low", "medium", "high", "critical"];

fn trimmed(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

#[derive(Debug, FromRow)]
pub struct MaintenanceRow {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub asset_tag: String,
    pub asset_name: String,
    pub title: String,
    pub description: Option<String>,
    pub maintenance_type: String,
    pub status: String,
    pub priority: String,
    pub supplier_id: Option<Uuid>,
    pub supplier_name: Option<String>,
    pub rma_reference: Option<String>,
    pub assigned_to_user_id: Option<Uuid>,
    pub assigned_to_user_name: Option<String>,
    pub previous_status_id: Option<Uuid>,
    pub previous_status_name: Option<String>,
    pub reported_at: DateTime<Utc>,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub completion_notes: Option<String>,
    pub cost: Option<Decimal>,
    pub currency: String,
    pub created_by_user_id: Option<Uuid>,
    pub created_by_user_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// `$1` organization, `$2` locale.
const MAINTENANCE_SELECT: &str = r#"
    SELECT m.id, m.asset_id, a.asset_tag,
           COALESCE(a.name, a.asset_tag) AS asset_name,
           m.title, m.description, m.maintenance_type, m.status, m.priority,
           m.supplier_id,
           COALESCE((SELECT s.name FROM supplier_translations s
                     WHERE s.supplier_id = m.supplier_id AND s.locale = $2),
                    (SELECT s.name FROM supplier_translations s
                     WHERE s.supplier_id = m.supplier_id AND s.locale = 'en')) AS supplier_name,
           m.rma_reference, m.assigned_to_user_id,
           (SELECT NULLIF(btrim(u.first_name || ' ' || u.last_name), '') FROM users u
            WHERE u.id = m.assigned_to_user_id) AS assigned_to_user_name,
           m.previous_status_id,
           COALESCE((SELECT st.name FROM status_translations st
                     WHERE st.status_id = m.previous_status_id AND st.locale = $2),
                    (SELECT st.name FROM status_translations st
                     WHERE st.status_id = m.previous_status_id AND st.locale = 'en')) AS previous_status_name,
           m.reported_at, m.scheduled_at, m.started_at, m.completed_at, m.completion_notes,
           m.cost, m.currency, m.created_by_user_id,
           (SELECT NULLIF(btrim(u.first_name || ' ' || u.last_name), '') FROM users u
            WHERE u.id = m.created_by_user_id) AS created_by_user_name,
           m.created_at, m.updated_at
    FROM asset_maintenances m
    JOIN assets a ON a.id = m.asset_id
    WHERE m.organization_id = $1
"#;

/// `$3` q, `$4` asset, `$5` status, `$6` type, `$7` priority.
const MAINTENANCE_FILTER: &str = r#"
      AND ($3::text IS NULL OR m.title ILIKE '%' || $3 || '%'
           OR a.asset_tag ILIKE '%' || $3 || '%'
           OR m.rma_reference ILIKE '%' || $3 || '%')
      AND ($4::uuid IS NULL OR m.asset_id = $4)
      AND ($5::text IS NULL OR m.status = $5)
      AND ($6::text IS NULL OR m.maintenance_type = $6)
      AND ($7::text IS NULL OR m.priority = $7)
"#;

pub async fn list_maintenances(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    query: &MaintenanceListQuery,
    page: u32,
    per_page: u32,
) -> Result<(Vec<MaintenanceRow>, i64)> {
    if let Some(status) = query.status.as_deref() {
        validate_enum("status", status, &MAINTENANCE_STATUSES)?;
    }
    if let Some(kind) = query.maintenance_type.as_deref() {
        validate_enum("maintenance_type", kind, &MAINTENANCE_TYPES)?;
    }
    if let Some(priority) = query.priority.as_deref() {
        validate_enum("priority", priority, &MAINTENANCE_PRIORITIES)?;
    }

    let sql = format!(
        "{MAINTENANCE_SELECT}{MAINTENANCE_FILTER}
         ORDER BY m.created_at DESC, m.id LIMIT $8 OFFSET $9"
    );
    let rows = sqlx::query_as::<_, MaintenanceRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(query.q.as_deref())
        .bind(query.asset_id)
        .bind(query.status.as_deref())
        .bind(query.maintenance_type.as_deref())
        .bind(query.priority.as_deref())
        .bind(per_page as i64)
        .bind(((page.saturating_sub(1)) * per_page) as i64)
        .fetch_all(pool)
        .await?;

    let count_sql = format!(
        "SELECT count(*) FROM asset_maintenances m
         JOIN assets a ON a.id = m.asset_id
         WHERE m.organization_id = $1{MAINTENANCE_FILTER}"
    );
    let total: i64 = sqlx::query_scalar(&count_sql)
        .bind(org)
        .bind(locale)
        .bind(query.q.as_deref())
        .bind(query.asset_id)
        .bind(query.status.as_deref())
        .bind(query.maintenance_type.as_deref())
        .bind(query.priority.as_deref())
        .fetch_one(pool)
        .await?;
    Ok((rows, total))
}

pub async fn get_maintenance(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    id: Uuid,
) -> Result<MaintenanceRow> {
    let sql = format!("{MAINTENANCE_SELECT} AND m.id = $3");
    sqlx::query_as::<_, MaintenanceRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("Maintenance"))
}

/// Resolve the status an asset should return to: the snapshot, else deployable.
async fn restore_target(
    conn: &mut sqlx::PgConnection,
    org: Uuid,
    previous_status_id: Option<Uuid>,
    override_id: Option<Uuid>,
) -> Result<Uuid> {
    if let Some(id) = override_id {
        ensure_org_ref(conn, "statuses", org, id, "restore_status_id").await?;
        return Ok(id);
    }
    if let Some(id) = previous_status_id {
        return Ok(id);
    }
    lookups::find_status_for_transition(conn, org, "ready_to_deploy", "deployable")
        .await?
        .ok_or_else(|| {
            ApiError::Conflict(
                "No deployable status is configured to restore the asset".to_string(),
            )
        })
}

async fn set_asset_status(
    conn: &mut sqlx::PgConnection,
    org: Uuid,
    asset_id: Uuid,
    status_id: Uuid,
) -> Result<()> {
    sqlx::query(
        "UPDATE assets SET status_id = $3, updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(asset_id)
    .bind(org)
    .bind(status_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

async fn audit_event(
    conn: &mut sqlx::PgConnection,
    org: Uuid,
    actor: Uuid,
    ip: &Option<String>,
    action: &str,
    asset_id: Uuid,
    metadata: serde_json::Value,
) -> Result<()> {
    audit::insert_tx(
        conn,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: Some(actor),
            action,
            entity_type: "maintenance",
            entity_id: Some(asset_id),
            metadata,
            ip_address: ip.as_deref(),
        },
    )
    .await?;
    Ok(())
}

pub async fn create_maintenance(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: &Option<String>,
    user_agent: Option<&str>,
    req: &CreateMaintenanceRequest,
) -> Result<MaintenanceRow> {
    let title = req.title.trim();
    if title.is_empty() {
        return Err(ApiError::bad_request(
            "title",
            "required",
            "title cannot be blank",
        ));
    }
    let maintenance_type = req.maintenance_type.as_deref().unwrap_or("corrective");
    validate_enum("maintenance_type", maintenance_type, &MAINTENANCE_TYPES)?;
    let priority = req.priority.as_deref().unwrap_or("medium");
    validate_enum("priority", priority, &MAINTENANCE_PRIORITIES)?;

    let mut tx = pool.begin().await?;
    audit::bind_actor(
        &mut tx,
        audit::Actor {
            user_id: Some(actor),
            ip_address: ip.as_deref(),
            user_agent,
        },
    )
    .await?;

    let previous_status_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT status_id FROM assets WHERE id = $1 AND organization_id = $2 FOR UPDATE",
    )
    .bind(req.asset_id)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(previous_status_id) = previous_status_id else {
        return Err(ApiError::bad_request(
            "asset_id",
            "not_found",
            "asset_id does not exist in this organization",
        ));
    };

    let open_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM asset_maintenances
         WHERE organization_id = $1 AND asset_id = $2 AND status IN ('open', 'in_progress'))",
    )
    .bind(org)
    .bind(req.asset_id)
    .fetch_one(&mut *tx)
    .await?;
    if open_exists {
        return Err(ApiError::Conflict(
            "This asset already has an open maintenance ticket".to_string(),
        ));
    }

    if let Some(supplier_id) = req.supplier_id {
        ensure_org_ref(&mut tx, "suppliers", org, supplier_id, "supplier_id").await?;
    }
    if let Some(user_id) = req.assigned_to_user_id {
        ensure_org_ref(&mut tx, "users", org, user_id, "assigned_to_user_id").await?;
    }
    if let Some(cost) = req.cost {
        if cost.is_sign_negative() {
            return Err(ApiError::bad_request(
                "cost",
                "invalid",
                "cost cannot be negative",
            ));
        }
    }

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO asset_maintenances
            (organization_id, asset_id, title, description, maintenance_type, priority,
             supplier_id, rma_reference, assigned_to_user_id, previous_status_id,
             scheduled_at, cost, currency, created_by_user_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, COALESCE($13, 'USD'), $14)
         RETURNING id",
    )
    .bind(org)
    .bind(req.asset_id)
    .bind(title)
    .bind(trimmed(&req.description))
    .bind(maintenance_type)
    .bind(priority)
    .bind(req.supplier_id)
    .bind(trimmed(&req.rma_reference))
    .bind(req.assigned_to_user_id)
    .bind(previous_status_id)
    .bind(req.scheduled_at)
    .bind(req.cost)
    .bind(
        req.currency
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(str::to_uppercase),
    )
    .bind(actor)
    .fetch_one(&mut *tx)
    .await?;

    let in_repair_id =
        lookups::find_status_for_transition(&mut tx, org, "being_repaired", "pending")
            .await?
            .ok_or_else(|| {
                ApiError::Conflict(
                    "No repair status is configured; seed the `being_repaired` status".to_string(),
                )
            })?;
    set_asset_status(&mut tx, org, req.asset_id, in_repair_id).await?;

    audit_event(
        &mut tx,
        org,
        actor,
        ip,
        "maintenance_opened",
        id,
        serde_json::json!({
            "asset_id": req.asset_id,
            "from_status_id": previous_status_id,
            "to_status_id": in_repair_id,
            "ticket_id": id,
        }),
    )
    .await?;

    tx.commit().await?;
    get_maintenance(pool, org, locale, id).await
}

pub async fn update_maintenance(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: &Option<String>,
    user_agent: Option<&str>,
    id: Uuid,
    req: &UpdateMaintenanceRequest,
) -> Result<MaintenanceRow> {
    if let Some(kind) = req.maintenance_type.as_deref() {
        validate_enum("maintenance_type", kind, &MAINTENANCE_TYPES)?;
    }
    if let Some(priority) = req.priority.as_deref() {
        validate_enum("priority", priority, &MAINTENANCE_PRIORITIES)?;
    }
    if req.title.as_deref().is_some_and(|t| t.trim().is_empty()) {
        return Err(ApiError::bad_request(
            "title",
            "required",
            "title cannot be blank",
        ));
    }
    if req.cost.flatten().is_some_and(|c| c.is_sign_negative()) {
        return Err(ApiError::bad_request(
            "cost",
            "invalid",
            "cost cannot be negative",
        ));
    }

    let mut tx = pool.begin().await?;
    audit::bind_actor(
        &mut tx,
        audit::Actor {
            user_id: Some(actor),
            ip_address: ip.as_deref(),
            user_agent,
        },
    )
    .await?;

    let current: Option<(String, Option<Uuid>, Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        "SELECT status, supplier_id, assigned_to_user_id, previous_status_id
         FROM asset_maintenances
         WHERE id = $1 AND organization_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((status, supplier_id, assigned_to, _)) = current else {
        return Err(ApiError::NotFound("Maintenance"));
    };
    if matches!(status.as_str(), "completed" | "cancelled") {
        return Err(ApiError::Conflict(
            "Closed maintenance tickets cannot be edited".to_string(),
        ));
    }

    if let Some(Some(supplier_id)) = req.supplier_id {
        ensure_org_ref(&mut tx, "suppliers", org, supplier_id, "supplier_id").await?;
    }
    if let Some(Some(user_id)) = req.assigned_to_user_id {
        ensure_org_ref(&mut tx, "users", org, user_id, "assigned_to_user_id").await?;
    }

    sqlx::query(
        "UPDATE asset_maintenances
         SET title = COALESCE($3, title),
             description = CASE WHEN $4 THEN $5 ELSE description END,
             maintenance_type = COALESCE($6, maintenance_type),
             priority = COALESCE($7, priority),
             supplier_id = CASE WHEN $8 THEN $9 ELSE supplier_id END,
             rma_reference = CASE WHEN $10 THEN $11 ELSE rma_reference END,
             assigned_to_user_id = CASE WHEN $12 THEN $13 ELSE assigned_to_user_id END,
             scheduled_at = CASE WHEN $14 THEN $15 ELSE scheduled_at END,
             cost = CASE WHEN $16 THEN $17 ELSE cost END,
             currency = COALESCE($18, currency),
             updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(id)
    .bind(org)
    .bind(req.title.as_deref().map(str::trim))
    .bind(req.description.is_some())
    .bind(trimmed(&req.description.clone().flatten()))
    .bind(req.maintenance_type.as_deref())
    .bind(req.priority.as_deref())
    .bind(req.supplier_id.is_some())
    .bind(req.supplier_id.flatten().or(supplier_id))
    .bind(req.rma_reference.is_some())
    .bind(trimmed(&req.rma_reference.clone().flatten()))
    .bind(req.assigned_to_user_id.is_some())
    .bind(req.assigned_to_user_id.flatten().or(assigned_to))
    .bind(req.scheduled_at.is_some())
    .bind(req.scheduled_at.flatten())
    .bind(req.cost.is_some())
    .bind(req.cost.flatten())
    .bind(
        req.currency
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(str::to_uppercase),
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    get_maintenance(pool, org, locale, id).await
}

pub async fn start_maintenance(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: &Option<String>,
    user_agent: Option<&str>,
    id: Uuid,
) -> Result<MaintenanceRow> {
    let mut tx = pool.begin().await?;
    audit::bind_actor(
        &mut tx,
        audit::Actor {
            user_id: Some(actor),
            ip_address: ip.as_deref(),
            user_agent,
        },
    )
    .await?;

    let status: Option<String> = sqlx::query_scalar(
        "SELECT status FROM asset_maintenances
         WHERE id = $1 AND organization_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(status) = status else {
        return Err(ApiError::NotFound("Maintenance"));
    };
    if status != "open" {
        return Err(ApiError::Conflict(format!(
            "Only open tickets can be started (current status: {status})"
        )));
    }

    sqlx::query(
        "UPDATE asset_maintenances
         SET status = 'in_progress', started_at = COALESCE(started_at, now()), updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(id)
    .bind(org)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    get_maintenance(pool, org, locale, id).await
}

/// Close a ticket and restore the asset status captured when it opened.
pub async fn close_maintenance(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: &Option<String>,
    user_agent: Option<&str>,
    id: Uuid,
    completed: bool,
    notes: Option<&str>,
    cost: Option<Decimal>,
    restore_status_id: Option<Uuid>,
) -> Result<MaintenanceRow> {
    let mut tx = pool.begin().await?;
    audit::bind_actor(
        &mut tx,
        audit::Actor {
            user_id: Some(actor),
            ip_address: ip.as_deref(),
            user_agent,
        },
    )
    .await?;

    let current: Option<(String, Uuid, Option<Uuid>)> = sqlx::query_as(
        "SELECT status, asset_id, previous_status_id FROM asset_maintenances
         WHERE id = $1 AND organization_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((status, asset_id, previous_status_id)) = current else {
        return Err(ApiError::NotFound("Maintenance"));
    };
    if matches!(status.as_str(), "completed" | "cancelled") {
        return Err(ApiError::Conflict(format!("Ticket is already {status}")));
    }

    let target_status_id =
        restore_target(&mut tx, org, previous_status_id, restore_status_id).await?;

    let next_status = if completed { "completed" } else { "cancelled" };
    sqlx::query(
        "UPDATE asset_maintenances
         SET status = $3, completed_at = now(),
             completion_notes = COALESCE($4, completion_notes),
             cost = COALESCE($5, cost), updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(id)
    .bind(org)
    .bind(next_status)
    .bind(notes.map(str::trim).filter(|n| !n.is_empty()))
    .bind(cost)
    .execute(&mut *tx)
    .await?;

    set_asset_status(&mut tx, org, asset_id, target_status_id).await?;

    audit_event(
        &mut tx,
        org,
        actor,
        ip,
        "maintenance_closed",
        id,
        serde_json::json!({
            "ticket_id": id,
            "asset_id": asset_id,
            "outcome": next_status,
            "restored_status_id": target_status_id,
            "notes": notes,
        }),
    )
    .await?;

    tx.commit().await?;
    get_maintenance(pool, org, locale, id).await
}

pub async fn delete_maintenance(
    pool: &PgPool,
    org: Uuid,
    actor: Uuid,
    ip: &Option<String>,
    user_agent: Option<&str>,
    id: Uuid,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    audit::bind_actor(
        &mut tx,
        audit::Actor {
            user_id: Some(actor),
            ip_address: ip.as_deref(),
            user_agent,
        },
    )
    .await?;

    let current: Option<(String, Uuid, Option<Uuid>)> = sqlx::query_as(
        "SELECT status, asset_id, previous_status_id FROM asset_maintenances
         WHERE id = $1 AND organization_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((status, asset_id, previous_status_id)) = current else {
        return Err(ApiError::NotFound("Maintenance"));
    };
    if !matches!(status.as_str(), "open" | "cancelled") {
        return Err(ApiError::Conflict(
            "Only open or cancelled tickets can be deleted".to_string(),
        ));
    }

    // A deleted open ticket must not leave the asset stuck in repair.
    if status == "open" {
        let target = restore_target(&mut tx, org, previous_status_id, None).await?;
        set_asset_status(&mut tx, org, asset_id, target).await?;
    }

    sqlx::query("DELETE FROM asset_maintenances WHERE id = $1 AND organization_id = $2")
        .bind(id)
        .bind(org)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(())
}

// ===========================================================================
// Schedule-driven work orders (event sourced)
// ===========================================================================

pub const WORK_ORDER_TYPES: [&str; 2] = ["preventive", "corrective"];
pub const WORK_ORDER_STATUSES: [&str; 3] = ["open", "in_progress", "done"];
pub const FREQUENCY_TYPES: [&str; 3] = ["days", "months", "operating_hours"];

type WorkOrderState = (
    String,
    Option<Uuid>,
    Option<Decimal>,
    Option<String>,
    Option<DateTime<Utc>>,
);
type ScheduleState = (
    String,
    i32,
    Option<DateTime<Utc>>,
    Option<Decimal>,
    bool,
    Option<String>,
);
type DueScheduleState = (
    Uuid,
    String,
    i32,
    Option<DateTime<Utc>>,
    Option<Decimal>,
    Decimal,
);

#[derive(Debug, FromRow)]
pub struct WorkOrderRow {
    pub id: Uuid,
    pub schedule_id: Option<Uuid>,
    pub asset_id: Uuid,
    pub asset_name: String,
    #[sqlx(rename = "type")]
    pub work_order_type: String,
    pub status: String,
    pub assigned_to: Option<Uuid>,
    pub assigned_to_name: Option<String>,
    pub due_date: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub cost: Option<Decimal>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub struct MaintenanceScheduleRow {
    pub id: Uuid,
    pub asset_id: Uuid,
    pub frequency_type: String,
    pub interval: i32,
    pub next_due_at: Option<DateTime<Utc>>,
    pub next_due_operating_hours: Option<Decimal>,
    pub is_active: bool,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub struct WorkOrderEventRow {
    pub id: Uuid,
    pub work_order_id: Uuid,
    pub event_type: String,
    pub actor_id: Option<Uuid>,
    pub actor_name: Option<String>,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub struct ScheduleEventRow {
    pub id: Uuid,
    pub schedule_id: Uuid,
    pub event_type: String,
    pub actor_id: Option<Uuid>,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// `$1` organization.
const WORK_ORDER_SELECT: &str = r#"
    SELECT w.id, w.schedule_id, w.asset_id, a.name AS asset_name, w.type,
           w.status, w.assigned_to,
           (SELECT NULLIF(btrim(u.first_name || ' ' || u.last_name), '')
              FROM users u WHERE u.id = w.assigned_to) AS assigned_to_name,
           w.due_date, w.completed_at, w.cost, w.notes, w.created_at, w.updated_at
    FROM work_orders w
    JOIN assets a ON a.id = w.asset_id
    WHERE a.organization_id = $1
"#;

/// `$2` status, `$3` asset, `$4` assignee, `$5` type.
const WORK_ORDER_FILTER: &str = r#"
      AND ($2::text IS NULL OR w.status = $2)
      AND ($3::uuid IS NULL OR w.asset_id = $3)
      AND ($4::uuid IS NULL OR w.assigned_to = $4)
      AND ($5::text IS NULL OR w.type = $5)
"#;

/// `$1` organization, `$2` asset.
const SCHEDULE_SELECT: &str = r#"
    SELECT s.id, s.asset_id, s.frequency_type, s.interval, s.next_due_at,
           s.next_due_operating_hours, s.is_active, s.notes, s.created_at, s.updated_at
    FROM maintenance_schedules s
    JOIN assets a ON a.id = s.asset_id
    WHERE a.organization_id = $1
"#;

fn validate_work_order_type(value: &str) -> Result<()> {
    validate_enum("type", value, &WORK_ORDER_TYPES)
}

fn validate_work_order_status(value: &str) -> Result<()> {
    validate_enum("status", value, &WORK_ORDER_STATUSES)
}

fn validate_frequency_type(value: &str) -> Result<()> {
    validate_enum("frequency_type", value, &FREQUENCY_TYPES)
}

/// Advance a schedule's due marker by one interval.
///
/// `days`/`months` move `next_due_at`; `operating_hours` moves
/// `next_due_operating_hours`. Chrono clamps month-end dates (Jan 31 + 1 month
/// becomes Feb 28/29), which is the desired maintenance behaviour.
pub fn advance_due(
    frequency_type: &str,
    interval: i32,
    next_due_at: Option<DateTime<Utc>>,
    next_due_operating_hours: Option<Decimal>,
) -> Result<(Option<DateTime<Utc>>, Option<Decimal>)> {
    validate_frequency_type(frequency_type)?;
    if interval <= 0 {
        return Err(ApiError::bad_request(
            "interval",
            "invalid",
            "interval must be greater than zero",
        ));
    }

    match frequency_type {
        "days" => {
            let current = next_due_at.ok_or_else(|| {
                ApiError::invalid("next_due_at is required for day-based schedules")
            })?;
            let next = current
                .checked_add_signed(Duration::days(interval as i64))
                .ok_or_else(|| ApiError::invalid("next_due_at is out of range"))?;
            Ok((Some(next), None))
        }
        "months" => {
            let current = next_due_at.ok_or_else(|| {
                ApiError::invalid("next_due_at is required for month-based schedules")
            })?;
            let next = current
                .checked_add_months(Months::new(interval as u32))
                .ok_or_else(|| ApiError::invalid("next_due_at is out of range"))?;
            Ok((Some(next), None))
        }
        "operating_hours" => {
            let current = next_due_operating_hours.ok_or_else(|| {
                ApiError::invalid(
                    "next_due_operating_hours is required for operating-hour schedules",
                )
            })?;
            Ok((None, Some(current + Decimal::from(interval))))
        }
        _ => unreachable!("validate_frequency_type rejects unknown values"),
    }
}

fn initial_due(
    frequency_type: &str,
    interval: i32,
    asset_hours: Decimal,
) -> Result<(Option<DateTime<Utc>>, Option<Decimal>)> {
    match frequency_type {
        "days" | "months" => advance_due(frequency_type, interval, Some(Utc::now()), None),
        "operating_hours" => advance_due("operating_hours", interval, None, Some(asset_hours)),
        _ => validate_frequency_type(frequency_type).map(|_| unreachable!()),
    }
}

async fn insert_work_order_event(
    conn: &mut sqlx::PgConnection,
    work_order_id: Uuid,
    event_type: &str,
    actor: Option<Uuid>,
    payload: serde_json::Value,
) -> Result<Uuid> {
    let id = sqlx::query_scalar(
        "INSERT INTO work_order_events (work_order_id, event_type, actor_id, payload)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(work_order_id)
    .bind(event_type)
    .bind(actor)
    .bind(payload)
    .fetch_one(&mut *conn)
    .await?;
    Ok(id)
}

async fn insert_schedule_event(
    conn: &mut sqlx::PgConnection,
    schedule_id: Uuid,
    event_type: &str,
    actor: Option<Uuid>,
    payload: serde_json::Value,
) -> Result<Uuid> {
    let id = sqlx::query_scalar(
        "INSERT INTO schedule_events (schedule_id, event_type, actor_id, payload)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(schedule_id)
    .bind(event_type)
    .bind(actor)
    .bind(payload)
    .fetch_one(&mut *conn)
    .await?;
    Ok(id)
}

pub async fn list_work_orders(
    pool: &PgPool,
    org: Uuid,
    query: &WorkOrderListQuery,
    page: u32,
    per_page: u32,
) -> Result<(Vec<WorkOrderRow>, i64)> {
    if let Some(status) = query.status.as_deref() {
        validate_work_order_status(status)?;
    }
    if let Some(kind) = query.work_order_type.as_deref() {
        validate_work_order_type(kind)?;
    }

    let sql = format!(
        "{WORK_ORDER_SELECT}{WORK_ORDER_FILTER}
         ORDER BY w.created_at DESC, w.id LIMIT $6 OFFSET $7"
    );
    let rows = sqlx::query_as::<_, WorkOrderRow>(&sql)
        .bind(org)
        .bind(query.status.as_deref())
        .bind(query.asset_id)
        .bind(query.assigned_to)
        .bind(query.work_order_type.as_deref())
        .bind(per_page as i64)
        .bind(((page.saturating_sub(1)) * per_page) as i64)
        .fetch_all(pool)
        .await?;

    let count_sql = format!(
        "SELECT count(*) FROM work_orders w
         JOIN assets a ON a.id = w.asset_id
         WHERE a.organization_id = $1{WORK_ORDER_FILTER}"
    );
    let total: i64 = sqlx::query_scalar(&count_sql)
        .bind(org)
        .bind(query.status.as_deref())
        .bind(query.asset_id)
        .bind(query.assigned_to)
        .bind(query.work_order_type.as_deref())
        .fetch_one(pool)
        .await?;

    Ok((rows, total))
}

pub async fn is_admin(pool: &PgPool, org: Uuid, user_id: Uuid) -> Result<bool> {
    let is_admin: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM user_roles ur
            JOIN roles r ON r.id = ur.role_id
            WHERE ur.organization_id = $1 AND ur.user_id = $2 AND r.code = 'admin'
         )",
    )
    .bind(org)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(is_admin)
}

pub async fn get_work_order(pool: &PgPool, org: Uuid, id: Uuid) -> Result<WorkOrderRow> {
    let sql = format!("{WORK_ORDER_SELECT} AND w.id = $2");
    sqlx::query_as::<_, WorkOrderRow>(&sql)
        .bind(org)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("Work order"))
}

pub async fn list_work_order_events(
    pool: &PgPool,
    org: Uuid,
    work_order_id: Uuid,
) -> Result<Vec<WorkOrderEventRow>> {
    let rows = sqlx::query_as::<_, WorkOrderEventRow>(
        r#"
        SELECT e.id, e.work_order_id, e.event_type, e.actor_id,
               (SELECT NULLIF(btrim(u.first_name || ' ' || u.last_name), '')
                  FROM users u WHERE u.id = e.actor_id) AS actor_name,
               e.payload, e.created_at
        FROM work_order_events e
        JOIN work_orders w ON w.id = e.work_order_id
        JOIN assets a ON a.id = w.asset_id
        WHERE a.organization_id = $1 AND e.work_order_id = $2
        ORDER BY e.created_at ASC, e.id ASC
        "#,
    )
    .bind(org)
    .bind(work_order_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_work_order_event(
    pool: &PgPool,
    org: Uuid,
    event_id: Uuid,
) -> Result<WorkOrderEventRow> {
    sqlx::query_as::<_, WorkOrderEventRow>(
        r#"
        SELECT e.id, e.work_order_id, e.event_type, e.actor_id,
               (SELECT NULLIF(btrim(u.first_name || ' ' || u.last_name), '')
                  FROM users u WHERE u.id = e.actor_id) AS actor_name,
               e.payload, e.created_at
        FROM work_order_events e
        JOIN work_orders w ON w.id = e.work_order_id
        JOIN assets a ON a.id = w.asset_id
        WHERE a.organization_id = $1 AND e.id = $2
        "#,
    )
    .bind(org)
    .bind(event_id)
    .fetch_optional(pool)
    .await?
    .ok_or(ApiError::NotFound("Work order event"))
}

#[allow(clippy::too_many_arguments)]
async fn create_work_order_inner(
    pool: &PgPool,
    org: Uuid,
    actor: Option<Uuid>,
    asset_id: Uuid,
    work_order_type: &str,
    notes: Option<String>,
    due_date: Option<DateTime<Utc>>,
    source: &str,
) -> Result<WorkOrderRow> {
    validate_work_order_type(work_order_type)?;
    let mut tx = pool.begin().await?;
    ensure_org_ref(&mut tx, "assets", org, asset_id, "asset_id").await?;

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO work_orders (asset_id, type, status, due_date, notes)
         VALUES ($1, $2, 'open', $3, $4) RETURNING id",
    )
    .bind(asset_id)
    .bind(work_order_type)
    .bind(due_date)
    .bind(trimmed(&notes))
    .fetch_one(&mut *tx)
    .await?;

    insert_work_order_event(
        &mut tx,
        id,
        "created",
        actor,
        serde_json::json!({ "source": source }),
    )
    .await?;

    tx.commit().await?;
    get_work_order(pool, org, id).await
}

pub async fn create_work_order(
    pool: &PgPool,
    org: Uuid,
    actor: Uuid,
    req: &CreateWorkOrderRequest,
) -> Result<WorkOrderRow> {
    create_work_order_inner(
        pool,
        org,
        Some(actor),
        req.asset_id,
        &req.work_order_type,
        req.notes.clone(),
        req.due_date,
        "manual",
    )
    .await
}

pub async fn report_breakdown(
    pool: &PgPool,
    org: Uuid,
    actor: Uuid,
    asset_id: Uuid,
    notes: Option<String>,
) -> Result<WorkOrderRow> {
    create_work_order_inner(
        pool,
        org,
        Some(actor),
        asset_id,
        "corrective",
        notes,
        Some(Utc::now()),
        "breakdown_report",
    )
    .await
}

pub async fn update_work_order(
    pool: &PgPool,
    org: Uuid,
    actor: Uuid,
    id: Uuid,
    req: &UpdateWorkOrderRequest,
) -> Result<WorkOrderRow> {
    if let Some(status) = req.status.as_deref() {
        validate_work_order_status(status)?;
    }
    if req.cost.is_some_and(|cost| cost.is_sign_negative()) {
        return Err(ApiError::bad_request(
            "cost",
            "invalid",
            "cost cannot be negative",
        ));
    }

    let mut tx = pool.begin().await?;
    let current: Option<WorkOrderState> = sqlx::query_as(
        "SELECT w.status, w.assigned_to, w.cost, w.notes, w.completed_at
             FROM work_orders w
             JOIN assets a ON a.id = w.asset_id
             WHERE w.id = $1 AND a.organization_id = $2
             FOR UPDATE OF w",
    )
    .bind(id)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((old_status, old_assigned, old_cost, old_notes, old_completed)) = current else {
        return Err(ApiError::NotFound("Work order"));
    };

    let new_status = req.status.clone().unwrap_or_else(|| old_status.clone());
    if new_status == "done" && req.cost.is_none() {
        return Err(ApiError::bad_request(
            "cost",
            "required",
            "cost is required when moving a work order to done",
        ));
    }

    let new_assigned = match req.assigned_to {
        Some(value) => value,
        None => old_assigned,
    };
    if let Some(user_id) = new_assigned {
        ensure_org_ref(&mut tx, "users", org, user_id, "assigned_to").await?;
    }
    let new_cost = req.cost.or(old_cost);
    let new_notes = match &req.notes {
        Some(value) => trimmed(value),
        None => old_notes.clone(),
    };
    let completed_at = if new_status == "done" {
        Some(Utc::now())
    } else if req.status.is_some() {
        None
    } else {
        old_completed
    };

    sqlx::query(
        "UPDATE work_orders
         SET assigned_to = $1, status = $2, cost = $3, notes = $4,
             completed_at = $5, updated_at = now()
         WHERE id = $6",
    )
    .bind(new_assigned)
    .bind(&new_status)
    .bind(new_cost)
    .bind(&new_notes)
    .bind(completed_at)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    if req.assigned_to.is_some() && new_assigned != old_assigned {
        insert_work_order_event(
            &mut tx,
            id,
            "assigned",
            Some(actor),
            serde_json::json!({ "from": old_assigned, "to": new_assigned }),
        )
        .await?;
    }
    if new_status != old_status {
        insert_work_order_event(
            &mut tx,
            id,
            "status_changed",
            Some(actor),
            serde_json::json!({ "from": old_status, "to": new_status }),
        )
        .await?;
    }
    if req.cost.is_some() && new_cost != old_cost {
        insert_work_order_event(
            &mut tx,
            id,
            "cost_recorded",
            Some(actor),
            serde_json::json!({ "cost": new_cost }),
        )
        .await?;
    }
    if req.notes.is_some() && new_notes != old_notes {
        insert_work_order_event(
            &mut tx,
            id,
            "note_added",
            Some(actor),
            serde_json::json!({ "note": new_notes }),
        )
        .await?;
    }
    if new_status == "done" && old_status != "done" {
        insert_work_order_event(
            &mut tx,
            id,
            "completed",
            Some(actor),
            serde_json::json!({ "cost": new_cost, "completed_at": completed_at }),
        )
        .await?;
    }

    tx.commit().await?;
    get_work_order(pool, org, id).await
}

pub async fn add_work_order_note(
    pool: &PgPool,
    org: Uuid,
    actor: Uuid,
    work_order_id: Uuid,
    req: &AddWorkOrderEventRequest,
) -> Result<WorkOrderEventRow> {
    let mut payload = req.payload.clone().unwrap_or_else(|| serde_json::json!({}));
    if let Some(note) = trimmed(&req.note) {
        if !payload.is_object() {
            payload = serde_json::json!({ "payload": payload });
        }
        payload["note"] = serde_json::Value::String(note);
    }
    if payload == serde_json::json!({}) {
        return Err(ApiError::bad_request(
            "note",
            "required",
            "provide a note or payload",
        ));
    }

    let mut tx = pool.begin().await?;
    let exists: Option<Uuid> = sqlx::query_scalar(
        "SELECT w.id FROM work_orders w
         JOIN assets a ON a.id = w.asset_id
         WHERE w.id = $1 AND a.organization_id = $2
         FOR UPDATE OF w",
    )
    .bind(work_order_id)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    if exists.is_none() {
        return Err(ApiError::NotFound("Work order"));
    }

    let event_id =
        insert_work_order_event(&mut tx, work_order_id, "note_added", Some(actor), payload).await?;
    sqlx::query("UPDATE work_orders SET updated_at = now() WHERE id = $1")
        .bind(work_order_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    get_work_order_event(pool, org, event_id).await
}

async fn asset_operating_hours(
    conn: &mut sqlx::PgConnection,
    org: Uuid,
    asset_id: Uuid,
) -> Result<Decimal> {
    sqlx::query_scalar::<_, Decimal>(
        "SELECT current_operating_hours FROM assets
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(asset_id)
    .bind(org)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(ApiError::NotFound("Asset"))
}

pub async fn list_schedules(
    pool: &PgPool,
    org: Uuid,
    asset_id: Uuid,
) -> Result<Vec<MaintenanceScheduleRow>> {
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

    let sql = format!(
        "{SCHEDULE_SELECT} AND s.asset_id = $2
         ORDER BY s.created_at DESC, s.id"
    );
    Ok(sqlx::query_as::<_, MaintenanceScheduleRow>(&sql)
        .bind(org)
        .bind(asset_id)
        .fetch_all(pool)
        .await?)
}

pub async fn get_schedule(pool: &PgPool, org: Uuid, id: Uuid) -> Result<MaintenanceScheduleRow> {
    let sql = format!("{SCHEDULE_SELECT} AND s.id = $2");
    sqlx::query_as::<_, MaintenanceScheduleRow>(&sql)
        .bind(org)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("Maintenance schedule"))
}

pub async fn create_schedule(
    pool: &PgPool,
    org: Uuid,
    actor: Uuid,
    asset_id: Uuid,
    req: &CreateMaintenanceScheduleRequest,
) -> Result<MaintenanceScheduleRow> {
    validate_frequency_type(&req.frequency_type)?;
    if req.interval <= 0 {
        return Err(ApiError::bad_request(
            "interval",
            "invalid",
            "interval must be greater than zero",
        ));
    }

    let mut tx = pool.begin().await?;
    let asset_hours = asset_operating_hours(&mut tx, org, asset_id).await?;
    let (next_due_at, next_due_operating_hours) =
        initial_due(&req.frequency_type, req.interval, asset_hours)?;

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO maintenance_schedules
            (asset_id, frequency_type, interval, next_due_at, next_due_operating_hours, notes)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(asset_id)
    .bind(&req.frequency_type)
    .bind(req.interval)
    .bind(next_due_at)
    .bind(next_due_operating_hours)
    .bind(trimmed(&req.notes))
    .fetch_one(&mut *tx)
    .await?;

    insert_schedule_event(
        &mut tx,
        id,
        "schedule_created",
        Some(actor),
        serde_json::json!({
            "frequency_type": req.frequency_type,
            "interval": req.interval,
            "next_due_at": next_due_at,
            "next_due_operating_hours": next_due_operating_hours,
        }),
    )
    .await?;

    tx.commit().await?;
    get_schedule(pool, org, id).await
}

pub async fn update_schedule(
    pool: &PgPool,
    org: Uuid,
    actor: Uuid,
    id: Uuid,
    req: &UpdateMaintenanceScheduleRequest,
) -> Result<MaintenanceScheduleRow> {
    if req.interval.is_some_and(|interval| interval <= 0) {
        return Err(ApiError::bad_request(
            "interval",
            "invalid",
            "interval must be greater than zero",
        ));
    }

    let mut tx = pool.begin().await?;
    let current: Option<ScheduleState> = sqlx::query_as(
        "SELECT s.frequency_type, s.interval, s.next_due_at,
                s.next_due_operating_hours, s.is_active, s.notes
         FROM maintenance_schedules s
         JOIN assets a ON a.id = s.asset_id
         WHERE s.id = $1 AND a.organization_id = $2
         FOR UPDATE OF s",
    )
    .bind(id)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((frequency_type, old_interval, old_due, old_hours, old_active, old_notes)) = current
    else {
        return Err(ApiError::NotFound("Maintenance schedule"));
    };

    let new_interval = req.interval.unwrap_or(old_interval);
    let new_due = req.next_due_at.or(old_due);
    let new_hours = req.next_due_operating_hours.or(old_hours);
    let new_active = req.is_active.unwrap_or(old_active);
    let new_notes = match &req.notes {
        Some(value) => trimmed(value),
        None => old_notes.clone(),
    };

    if matches!(frequency_type.as_str(), "days" | "months") && new_due.is_none() {
        return Err(ApiError::bad_request(
            "next_due_at",
            "required",
            "next_due_at is required for day/month schedules",
        ));
    }
    if frequency_type == "operating_hours" && new_hours.is_none() {
        return Err(ApiError::bad_request(
            "next_due_operating_hours",
            "required",
            "next_due_operating_hours is required for operating-hour schedules",
        ));
    }

    sqlx::query(
        "UPDATE maintenance_schedules
         SET interval = $1, next_due_at = $2, next_due_operating_hours = $3,
             is_active = $4, notes = $5, updated_at = now()
         WHERE id = $6",
    )
    .bind(new_interval)
    .bind(new_due)
    .bind(new_hours)
    .bind(new_active)
    .bind(&new_notes)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    insert_schedule_event(
        &mut tx,
        id,
        "schedule_updated",
        Some(actor),
        serde_json::json!({
            "interval": { "from": old_interval, "to": new_interval },
            "next_due_at": { "from": old_due, "to": new_due },
            "next_due_operating_hours": { "from": old_hours, "to": new_hours },
            "is_active": { "from": old_active, "to": new_active },
        }),
    )
    .await?;

    tx.commit().await?;
    get_schedule(pool, org, id).await
}

pub async fn deactivate_schedule(pool: &PgPool, org: Uuid, actor: Uuid, id: Uuid) -> Result<()> {
    let mut tx = pool.begin().await?;
    let exists: Option<Uuid> = sqlx::query_scalar(
        "SELECT s.id FROM maintenance_schedules s
         JOIN assets a ON a.id = s.asset_id
         WHERE s.id = $1 AND a.organization_id = $2
         FOR UPDATE OF s",
    )
    .bind(id)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    if exists.is_none() {
        return Err(ApiError::NotFound("Maintenance schedule"));
    }

    sqlx::query(
        "UPDATE maintenance_schedules
         SET is_active = FALSE, updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;

    insert_schedule_event(
        &mut tx,
        id,
        "schedule_deactivated",
        Some(actor),
        serde_json::json!({ "is_active": false }),
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Process every due schedule once. Used by the cron job and directly by tests.
pub async fn run_once(pool: &PgPool) -> std::result::Result<SchedulerRunSummary, ApiError> {
    let now = Utc::now();
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT s.id
         FROM maintenance_schedules s
         JOIN assets a ON a.id = s.asset_id
         WHERE s.is_active = TRUE
           AND (
                (s.frequency_type IN ('days', 'months')
                 AND s.next_due_at IS NOT NULL AND s.next_due_at <= $1)
             OR (s.frequency_type = 'operating_hours'
                 AND s.next_due_operating_hours IS NOT NULL
                 AND s.next_due_operating_hours <= a.current_operating_hours)
           )
         ORDER BY s.id",
    )
    .bind(now)
    .fetch_all(pool)
    .await?;

    let mut summary = SchedulerRunSummary::default();
    for id in ids {
        match generate_for_schedule(pool, id, now).await {
            Ok(true) => summary.generated += 1,
            Ok(false) => summary.skipped += 1,
            Err(error) => {
                summary.errors += 1;
                log::error!("maintenance scheduler failed for schedule {id}: {error}");
            }
        }
    }

    log::info!(
        "maintenance scheduler: generated={}, skipped={}, errors={}",
        summary.generated,
        summary.skipped,
        summary.errors
    );
    Ok(summary)
}

async fn generate_for_schedule(
    pool: &PgPool,
    schedule_id: Uuid,
    now: DateTime<Utc>,
) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let current: Option<DueScheduleState> = sqlx::query_as(
        "SELECT s.asset_id, s.frequency_type, s.interval, s.next_due_at,
                s.next_due_operating_hours, a.current_operating_hours
         FROM maintenance_schedules s
         JOIN assets a ON a.id = s.asset_id
         WHERE s.id = $1 AND s.is_active = TRUE
         FOR UPDATE OF s",
    )
    .bind(schedule_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((asset_id, frequency_type, interval, old_due, old_hours, current_hours)) = current
    else {
        tx.commit().await?;
        return Ok(false);
    };

    let is_due = match frequency_type.as_str() {
        "days" | "months" => old_due.is_some_and(|due| due <= now),
        "operating_hours" => old_hours.is_some_and(|hours| hours <= current_hours),
        _ => false,
    };
    if !is_due {
        tx.commit().await?;
        return Ok(false);
    }

    let due_date = if matches!(frequency_type.as_str(), "days" | "months") {
        old_due
    } else {
        None
    };

    let existing: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM work_orders
            WHERE schedule_id = $1
              AND due_date IS NOT DISTINCT FROM $2
         )",
    )
    .bind(schedule_id)
    .bind(due_date)
    .fetch_one(&mut *tx)
    .await?;

    let mut generated = false;
    if !existing {
        let work_order_id: Uuid = sqlx::query_scalar(
            "INSERT INTO work_orders
                (schedule_id, asset_id, type, status, due_date)
             VALUES ($1, $2, 'preventive', 'open', $3)
             RETURNING id",
        )
        .bind(schedule_id)
        .bind(asset_id)
        .bind(due_date)
        .fetch_one(&mut *tx)
        .await?;

        insert_work_order_event(
            &mut tx,
            work_order_id,
            "created",
            None,
            serde_json::json!({
                "source": "scheduler",
                "schedule_id": schedule_id,
            }),
        )
        .await?;
        generated = true;
    }

    let (new_due, new_hours) = advance_due(&frequency_type, interval, old_due, old_hours)?;

    sqlx::query(
        "UPDATE maintenance_schedules
         SET next_due_at = $1, next_due_operating_hours = $2, updated_at = now()
         WHERE id = $3",
    )
    .bind(new_due)
    .bind(new_hours)
    .bind(schedule_id)
    .execute(&mut *tx)
    .await?;

    insert_schedule_event(
        &mut tx,
        schedule_id,
        "schedule_advanced",
        None,
        serde_json::json!({
            "frequency_type": frequency_type,
            "from": old_due,
            "to": new_due,
            "from_operating_hours": old_hours,
            "to_operating_hours": new_hours,
        }),
    )
    .await?;

    tx.commit().await?;
    Ok(generated)
}

fn history_kind_rank(kind: &str) -> u8 {
    match kind {
        "completed" => 0,
        "status_changed" => 1,
        "cost_recorded" => 2,
        "assigned" => 3,
        "note_added" => 4,
        "work_order_created" => 5,
        "schedule_advanced" => 6,
        "schedule_updated" => 7,
        "schedule_deactivated" => 8,
        "schedule_created" => 9,
        _ => 10,
    }
}

fn event_summary(event_type: &str, payload: &serde_json::Value) -> String {
    match event_type {
        "assigned" => "Work order assigned".to_string(),
        "status_changed" => format!(
            "Status changed from {} to {}",
            payload["from"].as_str().unwrap_or("?"),
            payload["to"].as_str().unwrap_or("?")
        ),
        "cost_recorded" => format!("Cost recorded: {}", payload["cost"]),
        "note_added" => "Note added".to_string(),
        "completed" => "Work order completed".to_string(),
        "created" => "Work order created".to_string(),
        other => other.replace('_', " "),
    }
}

fn schedule_event_summary(event_type: &str) -> String {
    match event_type {
        "schedule_created" => "Maintenance schedule created".to_string(),
        "schedule_updated" => "Maintenance schedule updated".to_string(),
        "schedule_advanced" => "Maintenance schedule advanced".to_string(),
        "schedule_deactivated" => "Maintenance schedule deactivated".to_string(),
        other => other.replace('_', " "),
    }
}

pub async fn maintenance_history(
    pool: &PgPool,
    org: Uuid,
    asset_id: Uuid,
    cursor: Option<DateTime<Utc>>,
    limit: u32,
) -> Result<Vec<MaintenanceHistoryItem>> {
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

    let work_order_sql = format!("{WORK_ORDER_SELECT} AND w.asset_id = $2");
    let work_orders = sqlx::query_as::<_, WorkOrderRow>(&work_order_sql)
        .bind(org)
        .bind(asset_id)
        .fetch_all(pool)
        .await?;

    let work_order_events = sqlx::query_as::<_, WorkOrderEventRow>(
        r#"
        SELECT e.id, e.work_order_id, e.event_type, e.actor_id,
               (SELECT NULLIF(btrim(u.first_name || ' ' || u.last_name), '')
                  FROM users u WHERE u.id = e.actor_id) AS actor_name,
               e.payload, e.created_at
        FROM work_order_events e
        JOIN work_orders w ON w.id = e.work_order_id
        JOIN assets a ON a.id = w.asset_id
        WHERE a.organization_id = $1 AND w.asset_id = $2
        ORDER BY e.created_at ASC, e.id ASC
        "#,
    )
    .bind(org)
    .bind(asset_id)
    .fetch_all(pool)
    .await?;

    let schedule_events = sqlx::query_as::<_, ScheduleEventRow>(
        r#"
        SELECT e.id, e.schedule_id, e.event_type, e.actor_id, e.payload, e.created_at
        FROM schedule_events e
        JOIN maintenance_schedules s ON s.id = e.schedule_id
        JOIN assets a ON a.id = s.asset_id
        WHERE a.organization_id = $1 AND s.asset_id = $2
        ORDER BY e.created_at ASC, e.id ASC
        "#,
    )
    .bind(org)
    .bind(asset_id)
    .fetch_all(pool)
    .await?;

    let mut items = Vec::new();
    for work_order in work_orders {
        items.push(MaintenanceHistoryItem {
            occurred_at: work_order.created_at,
            kind: "work_order_created".to_string(),
            work_order_id: Some(work_order.id),
            schedule_id: work_order.schedule_id,
            summary: "Work order created".to_string(),
            payload: serde_json::json!({
                "type": work_order.work_order_type,
                "status": work_order.status,
                "assigned_to": work_order.assigned_to,
                "due_date": work_order.due_date,
                "completed_at": work_order.completed_at,
                "cost": work_order.cost,
                "notes": work_order.notes,
            }),
        });
    }
    for event in work_order_events {
        // The work_order_created row above already represents this event.
        if event.event_type == "created" {
            continue;
        }
        items.push(MaintenanceHistoryItem {
            occurred_at: event.created_at,
            kind: event.event_type.clone(),
            work_order_id: Some(event.work_order_id),
            schedule_id: None,
            summary: event_summary(&event.event_type, &event.payload),
            payload: event.payload,
        });
    }
    for event in schedule_events {
        items.push(MaintenanceHistoryItem {
            occurred_at: event.created_at,
            kind: event.event_type.clone(),
            work_order_id: None,
            schedule_id: Some(event.schedule_id),
            summary: schedule_event_summary(&event.event_type),
            payload: event.payload,
        });
    }

    if let Some(cursor) = cursor {
        items.retain(|item| item.occurred_at < cursor);
    }
    items.sort_by(|a, b| {
        b.occurred_at
            .cmp(&a.occurred_at)
            .then_with(|| history_kind_rank(&a.kind).cmp(&history_kind_rank(&b.kind)))
            .then_with(|| b.work_order_id.cmp(&a.work_order_id))
            .then_with(|| b.schedule_id.cmp(&a.schedule_id))
    });
    items.truncate(limit as usize);
    Ok(items)
}

#[cfg(test)]
mod schedule_tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(year: i32, month: u32, day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, 6, 0, 0).unwrap()
    }

    #[test]
    fn advances_days() {
        let (next, hours) = advance_due("days", 30, Some(utc(2026, 1, 1)), None).unwrap();
        assert_eq!(next, Some(utc(2026, 1, 31)));
        assert_eq!(hours, None);
    }

    #[test]
    fn advances_months_and_clamps_month_end() {
        let (next, _) = advance_due("months", 1, Some(utc(2026, 1, 31)), None).unwrap();
        assert_eq!(next, Some(utc(2026, 2, 28)));

        let (leap, _) = advance_due("months", 1, Some(utc(2024, 1, 31)), None).unwrap();
        assert_eq!(leap, Some(utc(2024, 2, 29)));
    }

    #[test]
    fn advances_operating_hours() {
        let (next, hours) =
            advance_due("operating_hours", 10, None, Some(Decimal::from(100))).unwrap();
        assert_eq!(next, None);
        assert_eq!(hours, Some(Decimal::from(110)));
    }

    #[test]
    fn rejects_zero_interval() {
        assert!(advance_due("days", 0, Some(utc(2026, 1, 1)), None).is_err());
    }
}
