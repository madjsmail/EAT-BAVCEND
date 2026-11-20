// Maintenance ticket data access.
//
// Opening a ticket captures the asset's current status in `previous_status_id`
// and parks the asset in `being_repaired`; closing (complete/cancel) restores
// that exact status. All writes run in a transaction with an audit row.

use chrono::{DateTime, Utc};
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
            ApiError::Conflict("No deployable status is configured to restore the asset".to_string())
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

    let in_repair_id = lookups::find_status_for_transition(&mut tx, org, "being_repaired", "pending")
        .await?
        .ok_or_else(|| {
            ApiError::Conflict("No repair status is configured; seed the `being_repaired` status".to_string())
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
    if req
        .title
        .as_deref()
        .is_some_and(|t| t.trim().is_empty())
    {
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
        return Err(ApiError::Conflict(format!(
            "Ticket is already {status}"
        )));
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
