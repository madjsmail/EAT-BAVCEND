use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde_json::json;

use crate::config::database::{Database, DatabaseTrait};
use crate::error::api_error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::rbac;
use crate::modules::organization::dto::{
    LOGO_CONTENT_TYPES, MAX_LOGO_BYTES, LogoUploadUrlRequest, LogoUploadUrlResponse, OrgSettings,
    OrganizationResponse, UpdateOrganizationRequest, is_supported_locale,
};
use crate::modules::organization::repo;
use crate::repository::audit;
use crate::response::api_response::{ApiResponse, ErrorDetail};
use crate::storage::garage;

fn field_error(field: &str, code: &str, message: &str) -> ErrorDetail {
    ErrorDetail {
        field: Some(field.to_string()),
        code: code.to_string(),
        message: message.to_string(),
    }
}

/// Parse the stored JSONB into the typed settings struct, falling back to
/// defaults if a legacy/hand-edited row does not match.
fn settings_of(org: &crate::entity::Organization) -> OrgSettings {
    match serde_json::from_value(org.settings.clone()) {
        Ok(settings) => settings,
        Err(e) => {
            log::warn!(
                "Organization {} has unreadable settings ({e}); using defaults",
                org.id
            );
            OrgSettings::default()
        }
    }
}

async fn to_response(
    org: crate::entity::Organization,
) -> Result<OrganizationResponse, ApiError> {
    let settings = settings_of(&org);
    // Signing is local, but a missing Garage config must not take the whole
    // settings page down: the key is still returned so the FE can retry.
    let logo_url = match org.logo_object_key.as_deref() {
        Some(key) => match garage::presigned_get(key).await {
            Ok(url) => Some(url),
            Err(e) => {
                log::warn!("Could not presign organization logo: {e}");
                None
            }
        },
        None => None,
    };
    Ok(OrganizationResponse {
        id: org.id,
        name: org.name,
        slug: org.slug,
        default_locale: org.default_locale,
        logo_object_key: org.logo_object_key,
        logo_url,
        settings,
        created_at: org.created_at,
        updated_at: org.updated_at,
    })
}

/// `GET /v1/api/organization`
pub async fn get(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
) -> Result<ApiResponse<OrganizationResponse>, ApiError> {
    rbac::require(&ctx, "organizations.read")?;
    let org = repo::find(db.get_pool(), ctx.organization_id())
        .await?
        .ok_or(ApiError::NotFound("Organization"))?;
    Ok(ApiResponse::success(to_response(org).await?))
}

/// `PATCH /v1/api/organization`
pub async fn update(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Json(payload): Json<UpdateOrganizationRequest>,
) -> Result<ApiResponse<OrganizationResponse>, ApiError> {
    rbac::require(&ctx, "organizations.update")?;

    let organization_id = ctx.organization_id();
    let mut errors = Vec::new();

    let name = payload.name.as_deref().map(str::trim);
    if name.is_some_and(|v| v.is_empty() || v.chars().count() > 120) {
        errors.push(field_error(
            "name",
            "invalid",
            "name must be between 1 and 120 characters",
        ));
    }

    let locale = payload.default_locale.as_deref().map(str::trim);
    if locale.is_some_and(|v| !is_supported_locale(v)) {
        errors.push(field_error(
            "default_locale",
            "invalid",
            "default_locale must be one of: en, fr, ar",
        ));
    }

    let logo_key = payload.logo_object_key.as_deref().map(str::trim);
    let tenant_prefix = format!("{organization_id}/");
    if logo_key.is_some_and(|v| {
        v.is_empty() || v.chars().count() > 500 || !v.starts_with(&tenant_prefix)
    }) {
        errors.push(field_error(
            "logo_object_key",
            "invalid",
            "logo_object_key must be a key issued for this organization",
        ));
    }

    if !errors.is_empty() {
        return Err(ApiError::Validation(errors));
    }

    let settings_patch = match payload.settings {
        Some(update) => update.into_patch()?,
        None => json!({}),
    };

    let pool = db.get_pool();
    let updated = repo::update(
        pool,
        organization_id,
        name,
        locale,
        logo_key,
        settings_patch,
    )
    .await?
    .ok_or(ApiError::NotFound("Organization"))?;

    audit::insert(
        pool,
        audit::Entry {
            organization_id: Some(organization_id),
            actor_user_id: Some(ctx.user.id),
            action: "organization_updated",
            entity_type: "organization",
            entity_id: Some(organization_id),
            metadata: json!({}),
            ip_address: None,
        },
    )
    .await?;

    Ok(ApiResponse::success(to_response(updated).await?))
}

/// `POST /v1/api/organization/logo/upload-url` — presigned PUT, then PATCH the
/// returned `object_key` into `logo_object_key`.
pub async fn logo_upload_url(
    ctx: AuthContext,
    Json(payload): Json<LogoUploadUrlRequest>,
) -> Result<ApiResponse<LogoUploadUrlResponse>, ApiError> {
    rbac::require(&ctx, "organizations.update")?;

    let mut errors = Vec::new();
    let file_name = payload.file_name.trim();
    if file_name.is_empty() || file_name.chars().count() > 180 {
        errors.push(field_error(
            "file_name",
            "invalid",
            "file_name must be between 1 and 180 characters",
        ));
    }
    if !LOGO_CONTENT_TYPES.contains(&payload.content_type.as_str()) {
        errors.push(field_error(
            "content_type",
            "invalid",
            "content_type must be image/png, image/jpeg or image/webp",
        ));
    }
    if !errors.is_empty() {
        return Err(ApiError::Validation(errors));
    }

    let organization_id = ctx.organization_id();
    let object_key = garage::object_key(organization_id, "logo", organization_id, file_name);
    let upload_url = garage::presigned_put(&object_key, &payload.content_type).await?;

    Ok(ApiResponse::success(LogoUploadUrlResponse {
        object_key,
        upload_url,
        expires_in_seconds: garage::PRESIGN_TTL_SECONDS,
        max_bytes: MAX_LOGO_BYTES,
    }))
}
