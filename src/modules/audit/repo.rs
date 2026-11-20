// Read-only audit log queries for the explorer timeline.

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::api_error::ApiError;
use crate::modules::audit::dto::AuditLogQuery;

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Debug, FromRow)]
pub struct AuditLogRow {
    pub id: Uuid,
    pub action: String,
    pub entity_type: String,
    pub entity_id: Option<Uuid>,
    pub actor_user_id: Option<Uuid>,
    pub actor_name: String,
    pub actor_email: Option<String>,
    pub ip_address: Option<String>,
    pub actor_user_agent: Option<String>,
    pub old_values: Option<serde_json::Value>,
    pub new_values: Option<serde_json::Value>,
    pub diff: Option<serde_json::Value>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// `$1` org, `$2` actor, `$3` entity type, `$4` entity id, `$5` events,
/// `$6` from, `$7` to, `$8` search.
const AUDIT_FILTER: &str = r#"
    WHERE a.organization_id = $1
      AND ($2::uuid IS NULL OR a.actor_user_id = $2)
      AND ($3::text IS NULL OR a.entity_type = $3)
      AND ($4::uuid IS NULL OR a.entity_id = $4)
      AND (cardinality($5::text[]) = 0 OR a.action = ANY($5))
      AND ($6::timestamptz IS NULL OR a.created_at >= $6)
      AND ($7::timestamptz IS NULL OR a.created_at <= $7)
      AND ($8::text IS NULL
           OR a.diff::text ILIKE '%' || $8 || '%'
           OR a.old_values::text ILIKE '%' || $8 || '%'
           OR a.new_values::text ILIKE '%' || $8 || '%'
           OR a.metadata::text ILIKE '%' || $8 || '%')
"#;

pub async fn list_audit_logs(
    pool: &PgPool,
    org: Uuid,
    query: &AuditLogQuery,
) -> Result<(Vec<AuditLogRow>, i64)> {
    let sql = format!(
        r#"
        SELECT a.id, a.action, a.entity_type, a.entity_id, a.actor_user_id,
               COALESCE(NULLIF(btrim(u.first_name || ' ' || u.last_name), ''), 'System') AS actor_name,
               u.email AS actor_email, a.ip_address, a.actor_user_agent,
               a.old_values, a.new_values, a.diff, a.metadata, a.created_at
        FROM audit_logs a
        LEFT JOIN users u ON u.id = a.actor_user_id
        {AUDIT_FILTER}
        ORDER BY a.created_at DESC, a.id DESC
        LIMIT $9 OFFSET $10
        "#
    );
    let rows = sqlx::query_as::<_, AuditLogRow>(&sql)
        .bind(org)
        .bind(query.actor_id)
        .bind(query.auditable_type.as_deref())
        .bind(query.auditable_id)
        .bind(&query.events)
        .bind(query.date_from)
        .bind(query.date_to)
        .bind(query.search.as_deref())
        .bind(query.per_page as i64)
        .bind(((query.page.saturating_sub(1)) * query.per_page) as i64)
        .fetch_all(pool)
        .await?;

    let count_sql = format!("SELECT count(*) FROM audit_logs a {AUDIT_FILTER}");
    let total: i64 = sqlx::query_scalar(&count_sql)
        .bind(org)
        .bind(query.actor_id)
        .bind(query.auditable_type.as_deref())
        .bind(query.auditable_id)
        .bind(&query.events)
        .bind(query.date_from)
        .bind(query.date_to)
        .bind(query.search.as_deref())
        .fetch_one(pool)
        .await?;

    Ok((rows, total))
}
