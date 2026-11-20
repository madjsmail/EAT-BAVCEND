// Audit Log Explorer query shapes.

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Default)]
pub struct AuditLogQuery {
    pub actor_id: Option<Uuid>,
    pub auditable_type: Option<String>,
    pub auditable_id: Option<Uuid>,
    /// Matched with `action = ANY(events)` when non-empty.
    pub events: Vec<String>,
    pub date_from: Option<DateTime<Utc>>,
    pub date_to: Option<DateTime<Utc>>,
    pub search: Option<String>,
    pub page: u32,
    pub per_page: u32,
}

#[derive(Debug, Serialize)]
pub struct FieldChange {
    pub field: String,
    pub old: serde_json::Value,
    pub new: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct AuditLogResponse {
    pub id: Uuid,
    /// Short verb: `created`, `updated`, `assigned`, `maintenance_opened`, ...
    pub event: String,
    pub auditable_type: String,
    pub auditable_id: Option<Uuid>,
    pub actor_id: Option<Uuid>,
    /// Resolved full name, `"System"` when there is no actor.
    pub actor_name: String,
    pub actor_email: Option<String>,
    pub actor_ip: Option<String>,
    pub actor_user_agent: Option<String>,
    pub old_values: Option<serde_json::Value>,
    pub new_values: Option<serde_json::Value>,
    pub diff: Option<serde_json::Value>,
    /// Flattened `diff` for timeline rendering.
    pub changes: Vec<FieldChange>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}
