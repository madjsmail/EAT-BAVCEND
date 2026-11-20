// Attachment handlers: multipart upload proxied to Garage, signed downloads.

use std::sync::Arc;

use axum::extract::{Multipart, Path, Query, State};
use axum::http::HeaderMap;
use uuid::Uuid;

use crate::config::database::{Database, DatabaseTrait};
use crate::error::api_error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::{client_ip, rbac, user_agent};
use crate::modules::attachments::dto::*;
use crate::modules::attachments::repo;
use crate::response::api_response::ApiResponse;
use crate::storage::garage;

/// Signed URLs are valid for 15 minutes (mirrors `storage::garage`).
const URL_TTL_SECONDS: i64 = 15 * 60;

fn attachment_response(row: repo::AttachmentRow) -> AttachmentResponse {
    AttachmentResponse {
        id: row.id,
        attachable_type: row.attachable_type,
        attachable_id: row.attachable_id,
        category: row.category,
        file_name: row.file_name,
        mime_type: row.mime_type,
        size_bytes: row.size_bytes,
        uploaded_by_user_id: row.uploaded_by_user_id,
        uploaded_by_user_name: row.uploaded_by_user_name,
        created_at: row.created_at,
    }
}

/// Reject executable and script payloads; documents and images are the use case.
fn mime_allowed(mime: &str) -> bool {
    mime.starts_with("image/")
        || mime == "application/pdf"
        || mime.starts_with("text/")
        || mime == "application/zip"
        || mime == "application/msword"
        || mime == "application/vnd.ms-excel"
        || mime.starts_with("application/vnd.openxmlformats-officedocument.")
}

pub async fn list_attachments(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Query(query): Query<AttachmentListQuery>,
) -> Result<ApiResponse<Vec<AttachmentResponse>>, ApiError> {
    rbac::require(&ctx, "attachments.read")?;
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(25).clamp(1, 100);
    let (rows, total) =
        repo::list_attachments(db.get_pool(), ctx.organization_id(), &query, page, per_page)
            .await?;
    Ok(ApiResponse::paginated(
        rows.into_iter().map(attachment_response).collect(),
        page,
        per_page,
        total as u64,
    ))
}

pub async fn upload_attachment(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<ApiResponse<AttachmentResponse>, ApiError> {
    rbac::require(&ctx, "attachments.write")?;

    let mut attachable_type: Option<String> = None;
    let mut attachable_id: Option<Uuid> = None;
    let mut category: Option<String> = None;
    let mut file_name: Option<String> = None;
    let mut mime_type: Option<String> = None;
    let mut bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad_request("file", "invalid", format!("Malformed multipart body: {e}")))?
    {
        match field.name().unwrap_or_default() {
            "attachable_type" => {
                attachable_type = Some(field.text().await.map_err(|e| {
                    ApiError::bad_request("attachable_type", "invalid", e.to_string())
                })?)
            }
            "attachable_id" => {
                let raw = field.text().await.map_err(|e| {
                    ApiError::bad_request("attachable_id", "invalid", e.to_string())
                })?;
                attachable_id = Some(raw.trim().parse::<Uuid>().map_err(|_| {
                    ApiError::bad_request("attachable_id", "invalid", "attachable_id is not a UUID")
                })?)
            }
            "category" => {
                category = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| ApiError::bad_request("category", "invalid", e.to_string()))?,
                )
            }
            "file" => {
                file_name = field.file_name().map(|n| n.trim().to_string());
                mime_type = field.content_type().map(|m| m.trim().to_string());
                bytes = Some(field.bytes().await.map_err(|e| {
                    ApiError::bad_request("file", "invalid", format!("Could not read file: {e}"))
                })?.to_vec());
            }
            _ => {
                // Drain unknown fields so the body parser stays in sync.
                let _ = field.bytes().await;
            }
        }
    }

    let attachable_type = attachable_type
        .map(|t| t.trim().to_lowercase())
        .ok_or_else(|| {
            ApiError::bad_request("attachable_type", "required", "attachable_type is required")
        })?;
    let attachable_id = attachable_id.ok_or_else(|| {
        ApiError::bad_request("attachable_id", "required", "attachable_id is required")
    })?;
    let file_name = file_name
        .filter(|n| !n.is_empty())
        .ok_or_else(|| ApiError::bad_request("file", "required", "file is required"))?;
    let bytes = bytes.ok_or_else(|| {
        ApiError::bad_request("file", "required", "file is required")
    })?;
    let mime_type = mime_type
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| "application/octet-stream".to_string());
    if !mime_allowed(&mime_type) {
        return Err(ApiError::bad_request(
            "file",
            "unsupported_type",
            format!("Files of type {mime_type} are not allowed"),
        ));
    }

    let ip = client_ip(&headers);
    let ua = user_agent(&headers);
    let row = repo::create_attachment(
        db.get_pool(),
        ctx.organization_id(),
        ctx.user.id,
        &ip,
        ua.as_deref(),
        &attachable_type,
        attachable_id,
        category.as_deref().map(str::trim).filter(|c| !c.is_empty()),
        &file_name,
        &mime_type,
        bytes,
    )
    .await?;

    Ok(ApiResponse::success_with_message(
        attachment_response(row),
        "Attachment uploaded",
    ))
}

pub async fn download_url(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<AttachmentUrlResponse>, ApiError> {
    rbac::require(&ctx, "attachments.read")?;
    let row = repo::get_attachment(db.get_pool(), ctx.organization_id(), id).await?;
    let url = garage::presigned_get(&row.storage_key).await?;
    Ok(ApiResponse::success(AttachmentUrlResponse {
        attachment_id: row.id,
        url,
        expires_in: URL_TTL_SECONDS,
    }))
}

pub async fn delete_attachment(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<String>, ApiError> {
    rbac::require(&ctx, "attachments.write")?;
    let ip = client_ip(&headers);
    let ua = user_agent(&headers);
    repo::delete_attachment(
        db.get_pool(),
        ctx.organization_id(),
        ctx.user.id,
        &ip,
        ua.as_deref(),
        id,
    )
    .await?;
    Ok(ApiResponse::success_with_message(
        id.to_string(),
        "Attachment deleted",
    ))
}
