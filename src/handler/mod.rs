// Thin handlers: validate -> RBAC -> repo -> audit -> JSON.
#![allow(dead_code)]

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, header};
use axum::response::IntoResponse;
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::config::database::{Database, DatabaseTrait};
use crate::dto::{
    AcceptInvitationRequest, CreateInvitationRequest, InvitationResponse, InvitePreview,
    LoginRequest, LoginResponse, SessionResponse, UserProfile,
};
use crate::error::api_error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::{client_ip, rbac, user_agent};
use crate::repository::{audit, invitation, organization, role, session, user};
use crate::response::api_response::ApiResponse;
use crate::service;

fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

fn normalize_slug(slug: &str) -> String {
    slug.trim().to_lowercase()
}

fn validate_email(email: &str) -> Result<(), ApiError> {
    let valid = email.len() <= 254
        && email.contains('@')
        && !email.starts_with('@')
        && !email.ends_with('@')
        && !email.contains(' ');
    if valid {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "email",
            "invalid",
            "Email is not valid",
        ))
    }
}

fn session_ttl() -> Duration {
    let hours = std::env::var("SESSION_TTL_HOURS")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(24 * 7);
    Duration::hours(hours.clamp(1, 24 * 30))
}

fn invite_base_url() -> String {
    std::env::var("INVITE_BASE_URL").unwrap_or_else(|_| {
        let host = std::env::var("APP_URL").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = std::env::var("APP_PORT").unwrap_or_else(|_| "8000".to_string());
        format!("http://{host}:{port}")
    })
}

// ---------------------------------------------------------------- auth ----

pub async fn login(
    State(db): State<Arc<Database>>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let pool = db.get_pool();
    let email = normalize_email(&payload.email);
    let slug = normalize_slug(&payload.organization_slug);
    let ip = client_ip(&headers);

    let org = organization::find_by_slug(pool, &slug)
        .await?
        .ok_or(ApiError::Unauthorized)?;

    let found = user::find_by_email(pool, org.id, &email).await?;

    let Some(user) = found else {
        audit::insert(
            pool,
            audit::Entry {
                organization_id: Some(org.id),
                actor_user_id: None,
                action: "login_failed",
                entity_type: "user",
                entity_id: None,
                metadata: json!({ "email": email, "reason": "unknown_user" }),
                ip_address: ip.as_deref(),
            },
        )
        .await?;
        return Err(ApiError::Unauthorized);
    };

    let password_ok = match user.password_hash.as_deref() {
        Some(hash) if user.status == "active" => service::verify_password(&payload.password, hash),
        Some(hash) => {
            // Still run the hash check so timing does not reveal account state.
            let _ = service::verify_password(&payload.password, hash);
            false
        }
        None => false,
    };

    if !password_ok {
        audit::insert(
            pool,
            audit::Entry {
                organization_id: Some(org.id),
                actor_user_id: Some(user.id),
                action: "login_failed",
                entity_type: "user",
                entity_id: Some(user.id),
                metadata: json!({ "email": email, "reason": "bad_credentials" }),
                ip_address: ip.as_deref(),
            },
        )
        .await?;
        return Err(ApiError::Unauthorized);
    }

    let token = service::generate_token();
    let expires_at = Utc::now() + session_ttl();
    let session_row = session::create(
        pool,
        session::NewSession {
            user_id: user.id,
            organization_id: org.id,
            token_hash: &service::hash_token(&token),
            ip_address: ip.as_deref(),
            user_agent: user_agent(&headers).as_deref(),
            expires_at,
        },
    )
    .await?;

    user::touch_login(pool, org.id, user.id).await?;
    let permissions = role::permissions_for_user(pool, org.id, user.id).await?;

    audit::insert(
        pool,
        audit::Entry {
            organization_id: Some(org.id),
            actor_user_id: Some(user.id),
            action: "login",
            entity_type: "session",
            entity_id: Some(session_row.id),
            metadata: json!({}),
            ip_address: ip.as_deref(),
        },
    )
    .await?;

    let cookie = service::session_cookie(
        &token,
        session_ttl().num_seconds(),
        service::cookie_secure(),
    );

    Ok((
        [(header::SET_COOKIE, cookie)],
        ApiResponse::success(LoginResponse {
            user: user.into(),
            permissions,
            session_id: session_row.id,
            expires_at,
        }),
    ))
}

pub async fn logout(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
) -> Result<impl IntoResponse, ApiError> {
    let pool = db.get_pool();
    session::revoke(pool, ctx.organization_id(), ctx.user.id, ctx.session.id).await?;
    audit::insert(
        pool,
        audit::Entry {
            organization_id: Some(ctx.organization_id()),
            actor_user_id: Some(ctx.user.id),
            action: "logout",
            entity_type: "session",
            entity_id: Some(ctx.session.id),
            metadata: json!({}),
            ip_address: None,
        },
    )
    .await?;

    Ok((
        [(
            header::SET_COOKIE,
            service::session_cookie("", 0, service::cookie_secure()),
        )],
        ApiResponse::success_with_message(json!({}), "Logged out"),
    ))
}

pub async fn me(ctx: AuthContext) -> ApiResponse<serde_json::Value> {
    let permissions = ctx.permissions.clone();
    let organization_id = ctx.organization_id();
    let session_id = ctx.session.id;
    ApiResponse::success(json!({
        "user": UserProfile::from(ctx.user),
        "permissions": permissions,
        "organization_id": organization_id,
        "session_id": session_id,
    }))
}

// --------------------------------------------------------- invitations ----

pub async fn create_invitation(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    headers: HeaderMap,
    Json(payload): Json<CreateInvitationRequest>,
) -> Result<ApiResponse<InvitationResponse>, ApiError> {
    rbac::require(&ctx, "invitations.create")?;
    let pool = db.get_pool();
    let email = normalize_email(&payload.email);
    validate_email(&email)?;

    let role = role::find_for_org(pool, ctx.organization_id(), payload.role_id)
        .await?
        .ok_or(ApiError::NotFound("Role"))?;

    if user::find_by_email(pool, ctx.organization_id(), &email)
        .await?
        .is_some()
    {
        return Err(ApiError::Conflict(
            "A user with this email already exists".to_string(),
        ));
    }
    if invitation::find_pending_by_email(pool, ctx.organization_id(), &email)
        .await?
        .is_some()
    {
        return Err(ApiError::Conflict(
            "A pending invitation already exists for this email".to_string(),
        ));
    }

    let hours = payload.expires_in_hours.unwrap_or(72).clamp(1, 24 * 30);
    let expires_at = Utc::now() + Duration::hours(hours);
    let token = service::generate_token();

    let created = invitation::create(
        pool,
        ctx.organization_id(),
        &email,
        payload.phone_number.as_deref(),
        role.id,
        &service::hash_token(&token),
        ctx.user.id,
        expires_at,
    )
    .await?;

    audit::insert(
        pool,
        audit::Entry {
            organization_id: Some(ctx.organization_id()),
            actor_user_id: Some(ctx.user.id),
            action: "invited",
            entity_type: "invitation",
            entity_id: Some(created.id),
            metadata: json!({ "email": email, "role": role.code }),
            ip_address: client_ip(&headers).as_deref(),
        },
    )
    .await?;

    Ok(ApiResponse::success(InvitationResponse {
        id: created.id,
        email: created.email,
        phone_number: created.phone_number,
        role_code: role.code,
        status: created.status,
        expires_at: created.expires_at,
        invite_link: format!("{}/invite/{token}", invite_base_url().trim_end_matches('/')),
    }))
}

pub async fn list_invitations(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
) -> Result<ApiResponse<serde_json::Value>, ApiError> {
    rbac::require(&ctx, "invitations.read")?;
    let rows = invitation::list_for_org(db.get_pool(), ctx.organization_id()).await?;
    Ok(ApiResponse::success(json!(rows)))
}

pub async fn list_roles(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
) -> Result<ApiResponse<serde_json::Value>, ApiError> {
    rbac::require(&ctx, "roles.read")?;
    let rows = role::list_for_org(db.get_pool(), ctx.organization_id()).await?;
    Ok(ApiResponse::success(json!(rows)))
}

pub async fn revoke_invitation(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<serde_json::Value>, ApiError> {
    rbac::require(&ctx, "invitations.revoke")?;
    let pool = db.get_pool();
    let revoked = invitation::revoke(pool, ctx.organization_id(), id)
        .await?
        .ok_or(ApiError::NotFound("Invitation"))?;

    audit::insert(
        pool,
        audit::Entry {
            organization_id: Some(ctx.organization_id()),
            actor_user_id: Some(ctx.user.id),
            action: "invitation_revoked",
            entity_type: "invitation",
            entity_id: Some(revoked.id),
            metadata: json!({ "email": revoked.email }),
            ip_address: None,
        },
    )
    .await?;

    Ok(ApiResponse::success_with_message(
        json!({}),
        "Invitation revoked",
    ))
}

/// Public: acceptance page lookup. No auth, token is the credential.
pub async fn preview_invitation(
    State(db): State<Arc<Database>>,
    Path(token): Path<String>,
) -> Result<ApiResponse<InvitePreview>, ApiError> {
    let pool = db.get_pool();
    let inv = invitation::find_by_token(pool, &service::hash_token(&token))
        .await?
        .ok_or(ApiError::NotFound("Invitation"))?;

    let status = if inv.status == "pending" && inv.expires_at <= Utc::now() {
        invitation::mark_status(pool, inv.id, "expired").await?;
        "expired".to_string()
    } else {
        inv.status.clone()
    };

    let org = organization::find_by_id(pool, inv.organization_id)
        .await?
        .ok_or(ApiError::NotFound("Organization"))?;
    let role = role::find_for_org(pool, inv.organization_id, inv.role_id)
        .await?
        .ok_or(ApiError::NotFound("Role"))?;

    Ok(ApiResponse::success(InvitePreview {
        organization_name: org.name,
        organization_slug: org.slug,
        email: inv.email,
        role_name: role.name,
        expires_at: inv.expires_at,
        status,
    }))
}

/// Public: consume the token, create/activate the user, assign the role.
pub async fn accept_invitation(
    State(db): State<Arc<Database>>,
    headers: HeaderMap,
    Json(payload): Json<AcceptInvitationRequest>,
) -> Result<ApiResponse<UserProfile>, ApiError> {
    if payload.password.len() < 8 {
        return Err(ApiError::bad_request(
            "password",
            "too_short",
            "Password must be at least 8 characters",
        ));
    }
    if payload.first_name.trim().is_empty() || payload.last_name.trim().is_empty() {
        return Err(ApiError::bad_request(
            "first_name",
            "required",
            "First and last name are required",
        ));
    }

    let pool = db.get_pool();
    let token_hash = service::hash_token(&payload.token);
    let preview = invitation::find_by_token(pool, &token_hash)
        .await?
        .ok_or(ApiError::NotFound("Invitation"))?;

    if preview.status != "pending" {
        return Err(ApiError::Conflict(
            "Invitation is no longer valid".to_string(),
        ));
    }
    if preview.expires_at <= Utc::now() {
        invitation::mark_status(pool, preview.id, "expired").await?;
        return Err(ApiError::bad_request(
            "token",
            "expired",
            "Invitation has expired",
        ));
    }

    let password_hash = service::hash_password(&payload.password).map_err(ApiError::Internal)?;
    let locale = payload.preferred_locale.as_deref().unwrap_or("en");
    let ip = client_ip(&headers);

    let mut tx: sqlx::Transaction<'_, sqlx::Postgres> = pool.begin().await?;

    // Re-read under lock: two concurrent accepts cannot both succeed.
    let inv = invitation::lock_by_token(&mut tx, &token_hash)
        .await?
        .ok_or(ApiError::NotFound("Invitation"))?;
    if inv.status != "pending" || inv.expires_at <= Utc::now() {
        return Err(ApiError::Conflict(
            "Invitation is no longer valid".to_string(),
        ));
    }

    let existing = user::find_by_email(pool, inv.organization_id, &inv.email).await?;
    let profile_user = match existing {
        Some(existing) if existing.status == "invited" => {
            user::activate(
                &mut tx,
                inv.organization_id,
                existing.id,
                payload.first_name.trim(),
                payload.last_name.trim(),
                payload.phone_number.as_deref(),
                payload.avatar_url.as_deref(),
                locale,
                &password_hash,
            )
            .await?
        }
        Some(_) => {
            return Err(ApiError::Conflict(
                "A user with this email already exists".to_string(),
            ));
        }
        None => {
            user::insert_active(
                &mut tx,
                inv.organization_id,
                &inv.email,
                payload.first_name.trim(),
                payload.last_name.trim(),
                payload.phone_number.as_deref(),
                payload.avatar_url.as_deref(),
                locale,
                &password_hash,
            )
            .await?
        }
    };

    role::assign(
        &mut tx,
        inv.organization_id,
        profile_user.id,
        inv.role_id,
        inv.invited_by,
    )
    .await?;
    invitation::accept(&mut tx, inv.organization_id, inv.id).await?;

    audit::insert_tx(
        &mut tx,
        audit::Entry {
            organization_id: Some(inv.organization_id),
            actor_user_id: Some(profile_user.id),
            action: "accepted",
            entity_type: "invitation",
            entity_id: Some(inv.id),
            metadata: json!({ "email": inv.email }),
            ip_address: ip.as_deref(),
        },
    )
    .await?;

    tx.commit().await?;

    Ok(ApiResponse::success_with_message(
        profile_user.into(),
        "Invitation accepted",
    ))
}

// ------------------------------------------------------------ sessions ----

pub async fn list_sessions(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
) -> Result<ApiResponse<Vec<SessionResponse>>, ApiError> {
    let rows = session::list_active(db.get_pool(), ctx.organization_id(), ctx.user.id).await?;
    let current = ctx.session.id;

    Ok(ApiResponse::success(
        rows.into_iter()
            .map(|s| SessionResponse {
                id: s.id,
                ip_address: s.ip_address,
                user_agent: s.user_agent,
                created_at: s.created_at,
                expires_at: s.expires_at,
                current: s.id == current,
            })
            .collect(),
    ))
}

pub async fn revoke_session(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let pool = db.get_pool();
    let audience = ctx.organization_id();
    let removed = session::revoke(pool, audience, ctx.user.id, id).await?;

    if !removed {
        // Not ours. If it belongs to another tenant, record the attempt.
        if let Some(owner_org) = session::organization_of(pool, id)
            .await?
            .filter(|owner_org| *owner_org != audience)
        {
            audit::insert(
                pool,
                audit::Entry {
                    organization_id: Some(audience),
                    actor_user_id: Some(ctx.user.id),
                    action: "cross_access_denied",
                    entity_type: "session",
                    entity_id: Some(id),
                    metadata: json!({ "target_organization_id": owner_org }),
                    ip_address: client_ip(&headers).as_deref(),
                },
            )
            .await?;
        }
        return Err(ApiError::NotFound("Session"));
    }

    audit::insert(
        pool,
        audit::Entry {
            organization_id: Some(audience),
            actor_user_id: Some(ctx.user.id),
            action: "session_revoked",
            entity_type: "session",
            entity_id: Some(id),
            metadata: json!({}),
            ip_address: client_ip(&headers).as_deref(),
        },
    )
    .await?;

    let mut response =
        ApiResponse::success_with_message(json!({}), "Session revoked").into_response();
    if id == ctx.session.id {
        response.headers_mut().insert(
            header::SET_COOKIE,
            service::session_cookie("", 0, service::cookie_secure())
                .parse()
                .map_err(|_| ApiError::Internal("Cookie header failed".to_string()))?,
        );
    }
    Ok(response)
}

pub async fn revoke_all_sessions(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
) -> Result<impl IntoResponse, ApiError> {
    let pool = db.get_pool();
    let revoked = session::revoke_all(pool, ctx.organization_id(), ctx.user.id).await?;

    audit::insert(
        pool,
        audit::Entry {
            organization_id: Some(ctx.organization_id()),
            actor_user_id: Some(ctx.user.id),
            action: "sessions_revoked",
            entity_type: "user",
            entity_id: Some(ctx.user.id),
            metadata: json!({ "revoked": revoked }),
            ip_address: None,
        },
    )
    .await?;

    Ok((
        [(
            header::SET_COOKIE,
            service::session_cookie("", 0, service::cookie_secure()),
        )],
        ApiResponse::success_with_message(json!({ "revoked": revoked }), "All sessions revoked"),
    ))
}

// --------------------------------------------------------------- users ----

#[derive(Debug, Deserialize)]
pub struct UpdateStatusRequest {
    pub status: String,
}

pub async fn update_user_status(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateStatusRequest>,
) -> Result<ApiResponse<UserProfile>, ApiError> {
    rbac::require(&ctx, "users.suspend")?;
    if !matches!(payload.status.as_str(), "active" | "suspended") {
        return Err(ApiError::bad_request(
            "status",
            "invalid",
            "Status must be 'active' or 'suspended'",
        ));
    }
    if id == ctx.user.id {
        return Err(ApiError::Conflict(
            "Cannot change your own status".to_string(),
        ));
    }

    let pool = db.get_pool();
    let updated = user::set_status(pool, ctx.organization_id(), id, &payload.status)
        .await?
        .ok_or(ApiError::NotFound("User"))?;

    audit::insert(
        pool,
        audit::Entry {
            organization_id: Some(ctx.organization_id()),
            actor_user_id: Some(ctx.user.id),
            action: "status_changed",
            entity_type: "user",
            entity_id: Some(updated.id),
            metadata: json!({ "status": payload.status }),
            ip_address: None,
        },
    )
    .await?;

    Ok(ApiResponse::success(updated.into()))
}
