// Attachment data access. Files live in Garage; PostgreSQL stores metadata only.

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::api_error::ApiError;
use crate::modules::attachments::dto::AttachmentListQuery;
use crate::modules::common::validate_enum;
use crate::repository::audit;
use crate::storage::garage;

type Result<T> = std::result::Result<T, ApiError>;

pub const ATTACHABLE_TYPES: [&str; 6] = [
    "asset",
    "product",
    "maintenance",
    "license",
    "accessory",
    "consumable",
];
pub const ATTACHMENT_CATEGORIES: [&str; 5] = [
    "invoice",
    "warranty",
    "damage_photo",
    "handover_receipt",
    "other",
];
/// Hard cap on the proxied upload body.
pub const MAX_UPLOAD_BYTES: usize = 20 * 1024 * 1024;

/// Physical table behind each polymorphic type. Internal constants only.
fn target_table(attachable_type: &str) -> Result<&'static str> {
    match attachable_type {
        "asset" => Ok("assets"),
        "product" => Ok("products"),
        "maintenance" => Ok("asset_maintenances"),
        "license" => Ok("licenses"),
        "accessory" => Ok("accessories"),
        "consumable" => Ok("consumables"),
        _ => Err(ApiError::bad_request(
            "attachable_type",
            "invalid",
            format!(
                "attachable_type must be one of: {}",
                ATTACHABLE_TYPES.join(", ")
            ),
        )),
    }
}

#[derive(Debug, FromRow)]
pub struct AttachmentRow {
    pub id: Uuid,
    pub attachable_type: String,
    pub attachable_id: Uuid,
    pub category: String,
    pub file_name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub uploaded_by_user_id: Option<Uuid>,
    pub uploaded_by_user_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub storage_key: String,
}

const ATTACHMENT_SELECT: &str = r#"
    SELECT a.id, a.attachable_type, a.attachable_id, a.category, a.file_name,
           a.mime_type, a.size_bytes, a.uploaded_by_user_id,
           (SELECT NULLIF(btrim(u.first_name || ' ' || u.last_name), '') FROM users u
            WHERE u.id = a.uploaded_by_user_id) AS uploaded_by_user_name,
           a.created_at, a.storage_key
    FROM attachments a
    WHERE a.organization_id = $1
"#;

pub async fn list_attachments(
    pool: &PgPool,
    org: Uuid,
    query: &AttachmentListQuery,
    page: u32,
    per_page: u32,
) -> Result<(Vec<AttachmentRow>, i64)> {
    if let Some(kind) = query.attachable_type.as_deref() {
        target_table(kind)?;
    }
    if let Some(category) = query.category.as_deref() {
        validate_enum("category", category, &ATTACHMENT_CATEGORIES)?;
    }

    let sql = format!(
        "{ATTACHMENT_SELECT}
         AND ($2::text IS NULL OR a.attachable_type = $2)
         AND ($3::uuid IS NULL OR a.attachable_id = $3)
         AND ($4::text IS NULL OR a.category = $4)
         ORDER BY a.created_at DESC, a.id LIMIT $5 OFFSET $6"
    );
    let rows = sqlx::query_as::<_, AttachmentRow>(&sql)
        .bind(org)
        .bind(query.attachable_type.as_deref())
        .bind(query.attachable_id)
        .bind(query.category.as_deref())
        .bind(per_page as i64)
        .bind(((page.saturating_sub(1)) * per_page) as i64)
        .fetch_all(pool)
        .await?;

    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM attachments a
         WHERE a.organization_id = $1
           AND ($2::text IS NULL OR a.attachable_type = $2)
           AND ($3::uuid IS NULL OR a.attachable_id = $3)
           AND ($4::text IS NULL OR a.category = $4)",
    )
    .bind(org)
    .bind(query.attachable_type.as_deref())
    .bind(query.attachable_id)
    .bind(query.category.as_deref())
    .fetch_one(pool)
    .await?;

    Ok((rows, total))
}

pub async fn get_attachment(pool: &PgPool, org: Uuid, id: Uuid) -> Result<AttachmentRow> {
    let sql = format!("{ATTACHMENT_SELECT} AND a.id = $2");
    sqlx::query_as::<_, AttachmentRow>(&sql)
        .bind(org)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("Attachment"))
}

#[allow(clippy::too_many_arguments)]
pub async fn create_attachment(
    pool: &PgPool,
    org: Uuid,
    actor: Uuid,
    ip: &Option<String>,
    user_agent: Option<&str>,
    attachable_type: &str,
    attachable_id: Uuid,
    category: Option<&str>,
    file_name: &str,
    mime_type: &str,
    bytes: Vec<u8>,
) -> Result<AttachmentRow> {
    let table = target_table(attachable_type)?;
    let category = category.unwrap_or("other");
    validate_enum("category", category, &ATTACHMENT_CATEGORIES)?;

    let exists: bool = sqlx::query_scalar(&format!(
        "SELECT EXISTS(SELECT 1 FROM {table} WHERE id = $1 AND organization_id = $2)"
    ))
    .bind(attachable_id)
    .bind(org)
    .fetch_one(pool)
    .await?;
    if !exists {
        return Err(ApiError::bad_request(
            "attachable_id",
            "not_found",
            "attachable_id does not exist in this organization",
        ));
    }

    let size_bytes = bytes.len() as i64;
    if size_bytes == 0 {
        return Err(ApiError::bad_request(
            "file",
            "required",
            "file cannot be empty",
        ));
    }
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(ApiError::bad_request(
            "file",
            "too_large",
            "file exceeds the 20 MB limit",
        ));
    }

    let key = garage::object_key(org, attachable_type, attachable_id, file_name);
    garage::put(&key, bytes, mime_type).await?;

    let inserted = async {
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
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO attachments
                (organization_id, attachable_type, attachable_id, category, file_name,
                 mime_type, size_bytes, storage_key, uploaded_by_user_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             RETURNING id",
        )
        .bind(org)
        .bind(attachable_type)
        .bind(attachable_id)
        .bind(category)
        .bind(file_name)
        .bind(mime_type)
        .bind(size_bytes)
        .bind(&key)
        .bind(actor)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok::<Uuid, ApiError>(id)
    }
    .await;

    let id = match inserted {
        Ok(id) => id,
        Err(e) => {
            // Don't leave an orphan object behind when the row cannot be written.
            if let Err(cleanup) = garage::delete(&key).await {
                log::error!("Could not remove orphaned object {key}: {cleanup}");
            }
            return Err(e);
        }
    };

    get_attachment(pool, org, id).await
}

pub async fn delete_attachment(
    pool: &PgPool,
    org: Uuid,
    actor: Uuid,
    ip: &Option<String>,
    user_agent: Option<&str>,
    id: Uuid,
) -> Result<()> {
    let row = get_attachment(pool, org, id).await?;

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
    sqlx::query("DELETE FROM attachments WHERE id = $1 AND organization_id = $2")
        .bind(id)
        .bind(org)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    if let Err(e) = garage::delete(&row.storage_key).await {
        // The metadata is gone; a leftover object is a cleanup task, not a 500.
        log::error!("Could not delete object {}: {e}", row.storage_key);
    }
    Ok(())
}
