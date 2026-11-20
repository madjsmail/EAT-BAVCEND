// Audit Log Explorer handler: one read-only, filterable timeline endpoint.

use std::sync::Arc;

use axum::extract::{RawQuery, State};
use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

use crate::config::database::{Database, DatabaseTrait};
use crate::error::api_error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::rbac;
use crate::modules::audit::dto::{AuditLogQuery, AuditLogResponse, FieldChange};
use crate::modules::audit::repo;
use crate::response::api_response::ApiResponse;

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn parse_event_list(value: &str) -> impl Iterator<Item = String> + '_ {
    value
        .split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// Accepts RFC 3339 timestamps and bare dates (`2026-01-31`).
fn parse_timestamp(field: &str, value: &str, end_of_day: bool) -> Result<DateTime<Utc>, ApiError> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Ok(parsed.with_timezone(&Utc));
    }
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| {
        ApiError::bad_request(
            field,
            "invalid",
            format!("{field} must be an ISO-8601 timestamp or a YYYY-MM-DD date"),
        )
    })?;
    let time = if end_of_day {
        date.and_hms_opt(23, 59, 59)
    } else {
        date.and_hms_opt(0, 0, 0)
    };
    let naive = time.ok_or_else(|| ApiError::bad_request(field, "invalid", "invalid date"))?;
    Ok(naive.and_utc())
}

/// `?event=created&event=updated` and `?event[]=...`, plus comma-separated values.
fn parse_query(raw: Option<&str>) -> Result<AuditLogQuery, ApiError> {
    let mut query = AuditLogQuery {
        page: 1,
        per_page: 25,
        ..Default::default()
    };

    for (key, value) in form_urlencoded::parse(raw.unwrap_or("").as_bytes()) {
        match key.as_ref() {
            "actor_id" | "actor" => {
                if let Some(v) = non_empty(&value) {
                    query.actor_id = Some(v.parse::<Uuid>().map_err(|_| {
                        ApiError::bad_request("actor_id", "invalid", "actor_id is not a UUID")
                    })?);
                }
            }
            "auditable_type" | "entity_type" => query.auditable_type = non_empty(&value),
            "auditable_id" | "entity_id" => {
                if let Some(v) = non_empty(&value) {
                    query.auditable_id = Some(v.parse::<Uuid>().map_err(|_| {
                        ApiError::bad_request("auditable_id", "invalid", "auditable_id is not a UUID")
                    })?);
                }
            }
            "event" | "event[]" | "events" | "events[]" => {
                query.events.extend(parse_event_list(&value));
            }
            "date_from" | "from" => {
                if let Some(v) = non_empty(&value) {
                    query.date_from = Some(parse_timestamp("date_from", &v, false)?);
                }
            }
            "date_to" | "to" => {
                if let Some(v) = non_empty(&value) {
                    query.date_to = Some(parse_timestamp("date_to", &v, true)?);
                }
            }
            "search" | "q" => query.search = non_empty(&value),
            "page" => query.page = value.parse().unwrap_or(1).max(1),
            "per_page" => query.per_page = value.parse().unwrap_or(25).clamp(1, 100),
            _ => {}
        }
    }

    if let (Some(from), Some(to)) = (query.date_from, query.date_to) {
        if from > to {
            return Err(ApiError::bad_request(
                "date_from",
                "invalid",
                "date_from must be earlier than date_to",
            ));
        }
    }

    Ok(query)
}

fn flatten_changes(diff: &Option<serde_json::Value>) -> Vec<FieldChange> {
    let Some(serde_json::Value::Object(fields)) = diff else {
        return Vec::new();
    };
    fields
        .iter()
        .map(|(field, delta)| FieldChange {
            field: field.clone(),
            old: delta.get("old").cloned().unwrap_or(serde_json::Value::Null),
            new: delta.get("new").cloned().unwrap_or(serde_json::Value::Null),
        })
        .collect()
}

fn log_response(row: repo::AuditLogRow) -> AuditLogResponse {
    let changes = flatten_changes(&row.diff);
    AuditLogResponse {
        id: row.id,
        event: row.action,
        auditable_type: row.entity_type,
        auditable_id: row.entity_id,
        actor_id: row.actor_user_id,
        actor_name: row.actor_name,
        actor_email: row.actor_email,
        actor_ip: row.ip_address,
        actor_user_agent: row.actor_user_agent,
        old_values: row.old_values,
        new_values: row.new_values,
        diff: row.diff,
        changes,
        metadata: row.metadata,
        created_at: row.created_at,
    }
}

pub async fn list_audit_logs(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    RawQuery(raw): RawQuery,
) -> Result<ApiResponse<Vec<AuditLogResponse>>, ApiError> {
    rbac::require(&ctx, "audit.read")?;
    let query = parse_query(raw.as_deref())?;

    let (rows, total) = repo::list_audit_logs(db.get_pool(), ctx.organization_id(), &query).await?;
    Ok(ApiResponse::paginated(
        rows.into_iter().map(log_response).collect(),
        query.page,
        query.per_page,
        total as u64,
    ))
}
