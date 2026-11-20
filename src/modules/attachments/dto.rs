// Attachment metadata: one row per uploaded file, polymorphic by type + id.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct AttachmentListQuery {
    /// `asset`, `product`, `maintenance`, `license`, `accessory` or `consumable`.
    pub attachable_type: Option<String>,
    pub attachable_id: Option<Uuid>,
    pub category: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct AttachmentResponse {
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
}

#[derive(Debug, Serialize)]
pub struct AttachmentUrlResponse {
    pub attachment_id: Uuid,
    pub url: String,
    /// Seconds until the signed URL expires.
    pub expires_in: i64,
}
