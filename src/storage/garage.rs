// Garage (S3-compatible) object storage: upload, presigned download, delete.
//
// The client is built once from environment variables and reused. Garage needs
// path-style addressing (`force_path_style(true)`), otherwise the bucket ends up
// in the host name and requests fail.

use std::sync::OnceLock;
use std::time::Duration;

use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use uuid::Uuid;

use crate::config::parameter;
use crate::error::api_error::ApiError;

/// Signed download URLs stay valid for 15 minutes.
const PRESIGN_TTL: Duration = Duration::from_secs(15 * 60);

/// Exposed so upload-url responses can tell the client how long they have.
pub const PRESIGN_TTL_SECONDS: u64 = 15 * 60;

static CLIENT: OnceLock<Client> = OnceLock::new();

fn env(name: &str) -> Result<String, ApiError> {
    parameter::get(name).map_err(ApiError::Internal)
}

fn client() -> Result<&'static Client, ApiError> {
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }

    let region = parameter::get("GARAGE_REGION").unwrap_or_else(|_| "garage".to_string());
    let credentials = Credentials::new(
        env("GARAGE_ACCESS_KEY_ID")?,
        env("GARAGE_SECRET_ACCESS_KEY")?,
        None,
        None,
        "garage",
    );
    let config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(credentials)
        .region(Region::new(region))
        .endpoint_url(env("GARAGE_ENDPOINT")?)
        .force_path_style(true)
        .build();

    let _ = CLIENT.set(Client::from_conf(config));
    CLIENT
        .get()
        .ok_or_else(|| ApiError::Internal("S3 client is not initialized".to_string()))
}

fn bucket() -> Result<String, ApiError> {
    env("GARAGE_BUCKET")
}

/// `{organization_id}/{entity_type}/{entity_id}/{uuid}-{sanitized_filename}`
pub fn object_key(org: Uuid, entity_type: &str, entity_id: Uuid, file_name: &str) -> String {
    format!(
        "{org}/{entity_type}/{entity_id}/{}-{}",
        Uuid::new_v4(),
        sanitize(file_name)
    )
}

fn sanitize(file_name: &str) -> String {
    let cleaned: String = file_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('.').trim_matches('_');
    if trimmed.is_empty() {
        "file".to_string()
    } else {
        trimmed.chars().take(180).collect()
    }
}

pub async fn put(key: &str, bytes: Vec<u8>, content_type: &str) -> Result<(), ApiError> {
    client()?
        .put_object()
        .bucket(bucket()?)
        .key(key)
        .content_type(content_type)
        .body(ByteStream::from(bytes))
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("Upload to storage failed: {e}")))?;
    Ok(())
}

/// Presigned PUT for a direct browser upload. The content type is baked into
/// the signature, so the client must send exactly the same header.
pub async fn presigned_put(key: &str, content_type: &str) -> Result<String, ApiError> {
    let presigning = PresigningConfig::expires_in(PRESIGN_TTL)
        .map_err(|e| ApiError::Internal(format!("Invalid presigning config: {e}")))?;
    let request = client()?
        .put_object()
        .bucket(bucket()?)
        .key(key)
        .content_type(content_type)
        .presigned(presigning)
        .await
        .map_err(|e| ApiError::Internal(format!("Could not sign the upload URL: {e}")))?;
    Ok(request.uri().to_string())
}

pub async fn presigned_get(key: &str) -> Result<String, ApiError> {    let presigning = PresigningConfig::expires_in(PRESIGN_TTL)
        .map_err(|e| ApiError::Internal(format!("Invalid presigning config: {e}")))?;
    let request = client()?
        .get_object()
        .bucket(bucket()?)
        .key(key)
        .presigned(presigning)
        .await
        .map_err(|e| ApiError::Internal(format!("Could not sign the download URL: {e}")))?;
    Ok(request.uri().to_string())
}

pub async fn delete(key: &str) -> Result<(), ApiError> {
    client()?
        .delete_object()
        .bucket(bucket()?)
        .key(key)
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("Delete from storage failed: {e}")))?;
    Ok(())
}
