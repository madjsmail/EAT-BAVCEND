// Organization settings queries. Tenant scope is the organization id itself.

use sqlx::PgPool;
use uuid::Uuid;

use crate::entity::Organization;

pub async fn find(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<Organization>> {
    sqlx::query_as::<_, Organization>("SELECT * FROM organizations WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Partial update: `None` keeps the current value, `settings` is merged into the
/// stored JSONB object (never replaced wholesale).
pub async fn update(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    default_locale: Option<&str>,
    logo_object_key: Option<&str>,
    settings: serde_json::Value,
) -> sqlx::Result<Option<Organization>> {
    sqlx::query_as::<_, Organization>(
        r#"
        UPDATE organizations
        SET name            = COALESCE($2, name),
            default_locale  = COALESCE($3, default_locale),
            logo_object_key = COALESCE($4, logo_object_key),
            settings        = settings || $5,
            updated_at      = now()
        WHERE id = $1
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(name)
    .bind(default_locale)
    .bind(logo_object_key)
    .bind(settings)
    .fetch_optional(pool)
    .await
}
