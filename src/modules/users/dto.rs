// Employee directory request/response shapes.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::modules::common::double_option;

#[derive(Debug, Default, Deserialize)]
pub struct ListUsersQuery {
    pub search: Option<String>,
    pub department_id: Option<Uuid>,
    pub location_id: Option<Uuid>,
    pub status: Option<String>,
    pub manager_id: Option<Uuid>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub full_name: String,
    pub employee_id: Option<String>,
    pub job_title: Option<String>,
    pub department_id: Option<Uuid>,
    pub department_name: Option<String>,
    pub location_id: Option<Uuid>,
    pub location_name: Option<String>,
    pub manager_id: Option<Uuid>,
    pub manager_name: Option<String>,
    pub preferred_locale: String,
    pub status: String,
    pub avatar_url: Option<String>,
    pub phone_number: Option<String>,
    pub email_verified_at: Option<DateTime<Utc>>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub employee_id: Option<String>,
    pub job_title: Option<String>,
    pub department_id: Option<Uuid>,
    pub location_id: Option<Uuid>,
    pub manager_id: Option<Uuid>,
    pub preferred_locale: Option<String>,
    pub phone_number: Option<String>,
    pub role_ids: Option<Vec<Uuid>>,
}

#[derive(Debug, Serialize)]
pub struct CreateUserResponse {
    pub user: UserResponse,
    /// Raw setup link, shown once so an admin can share it.
    pub setup_link: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Default, Deserialize)]
pub struct UpdateUserRequest {
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub employee_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub job_title: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub department_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub location_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub manager_id: Option<Option<Uuid>>,
    pub preferred_locale: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub phone_number: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub avatar_url: Option<Option<String>>,
}

#[derive(Debug, Default, Deserialize)]
pub struct OffboardRequest {
    pub new_manager_id: Option<Uuid>,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OffboardResponse {
    pub user: UserResponse,
    pub reassigned_reports: u64,
    pub return_requests_created: u64,
    pub pending_returns: Vec<PendingReturnResponse>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PendingReturnResponse {
    pub id: Uuid,
    pub entity_type: String,
    pub entity_id: Uuid,
    pub label: Option<String>,
    pub quantity: Option<i32>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ImportRowError {
    pub row: usize,
    pub email: Option<String>,
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct ImportSummary {
    pub total_rows: usize,
    pub imported: usize,
    pub failed: usize,
    pub errors: Vec<ImportRowError>,
}
