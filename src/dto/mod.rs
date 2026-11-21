// Request/response shapes for the auth + invitation endpoints.
#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Password login. `organization_slug` is deprecated: the tenant is resolved
/// from the user record, and the slug is only needed when the same email exists
/// in more than one organization.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub organization_slug: Option<String>,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct UserProfile {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub phone_number: Option<String>,
    pub avatar_url: Option<String>,
    pub status: String,
    pub preferred_locale: String,
    pub auth_provider: String,
    pub last_login_at: Option<DateTime<Utc>>,
}

impl From<crate::entity::User> for UserProfile {
    fn from(u: crate::entity::User) -> Self {
        Self {
            id: u.id,
            organization_id: u.organization_id,
            email: u.email,
            first_name: u.first_name,
            last_name: u.last_name,
            phone_number: u.phone_number,
            avatar_url: u.avatar_url,
            status: u.status,
            preferred_locale: u.preferred_locale,
            auth_provider: u.auth_provider,
            last_login_at: u.last_login_at,
        }
    }
}

/// Email-first sign-in request; `organization_slug` is optional for the same
/// reason as in `LoginRequest`.
#[derive(Debug, Deserialize)]
pub struct LoginRequestBody {
    pub organization_slug: Option<String>,
    pub email: String,
}

#[derive(Debug, Deserialize)]
pub struct SetupAccountRequest {
    pub token: String,
    pub password: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone_number: Option<String>,
    pub preferred_locale: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub user: UserProfile,
    pub permissions: Vec<String>,
    pub session_id: Uuid,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateInvitationRequest {
    pub email: String,
    pub phone_number: Option<String>,
    pub role_id: Uuid,
    pub expires_in_hours: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct InvitationResponse {
    pub id: Uuid,
    pub email: String,
    pub phone_number: Option<String>,
    pub role_code: String,
    pub status: String,
    pub expires_at: DateTime<Utc>,
    /// Single-use link to share over WhatsApp/email. The raw token is shown once.
    pub invite_link: String,
}

#[derive(Debug, Serialize)]
pub struct InvitePreview {
    pub organization_name: String,
    pub organization_slug: String,
    pub email: String,
    pub role_name: String,
    pub expires_at: DateTime<Utc>,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct AcceptInvitationRequest {
    pub token: String,
    pub password: String,
    pub first_name: String,
    pub last_name: String,
    pub phone_number: Option<String>,
    pub avatar_url: Option<String>,
    pub preferred_locale: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SessionResponse {
    pub id: Uuid,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub current: bool,
    pub is_current: bool,
}

/// Full self profile returned by `GET /v1/api/me`.
#[derive(Debug, Serialize)]
pub struct ProfileResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    /// Convenience full name: `"{first_name} {last_name}"`.
    pub name: String,
    pub phone_number: Option<String>,
    pub avatar_url: Option<String>,
    pub status: String,
    pub preferred_locale: String,
    pub auth_provider: String,
    pub employee_id: Option<String>,
    pub job_title: Option<String>,
    pub roles: Vec<String>,
    pub role: Option<String>,
    pub email_verified_at: Option<DateTime<Utc>>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ProfileResponse {
    pub fn new(u: crate::entity::User, roles: Vec<String>) -> Self {
        Self {
            id: u.id,
            organization_id: u.organization_id,
            email: u.email,
            name: format!("{} {}", u.first_name, u.last_name).trim().to_string(),
            first_name: u.first_name,
            last_name: u.last_name,
            phone_number: u.phone_number,
            avatar_url: u.avatar_url,
            status: u.status,
            preferred_locale: u.preferred_locale,
            auth_provider: u.auth_provider,
            employee_id: u.employee_id,
            job_title: u.job_title,
            role: roles.first().cloned(),
            roles,
            email_verified_at: u.email_verified_at,
            last_login_at: u.last_login_at,
            created_at: u.created_at,
            updated_at: u.updated_at,
        }
    }
}

/// `PATCH /v1/api/me`. Email, role, status and tenant are not self-editable.
#[derive(Debug, Deserialize)]
pub struct UpdateProfileRequest {
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone_number: Option<String>,
    pub avatar_url: Option<String>,
    pub preferred_locale: Option<String>,
}

/// `POST /v1/api/me/password`.
#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}
