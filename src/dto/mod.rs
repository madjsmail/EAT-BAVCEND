// Request/response shapes for the auth + invitation endpoints.
#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub organization_slug: String,
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

#[derive(Debug, Deserialize)]
pub struct LoginRequestBody {
    pub organization_slug: String,
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
    pub expires_at: DateTime<Utc>,
    pub current: bool,
}
