// Milestone 1 acceptance tests: invite-only auth, sessions, tenant isolation.
//
// Each test gets an isolated database (migrations are applied by sqlx::test).

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use rust_api::config::database::Database;
use rust_api::routes::root::app;
use rust_api::service::{generate_token, hash_password, hash_token};

struct Tenant {
    org_id: Uuid,
    user_id: Uuid,
    session_id: Uuid,
    token: String,
}

fn router(pool: &PgPool) -> Router {
    app(Arc::new(Database::from_pool(pool.clone())))
}

/// Insert an org + active admin + live session directly (bootstrap equivalent).
async fn seed_tenant(pool: &PgPool, slug: &str, email: &str, role_code: &str) -> Tenant {
    let org_id: Uuid =
        sqlx::query_scalar("INSERT INTO organizations (name, slug) VALUES ($1, $2) RETURNING id")
            .bind(format!("Org {slug}"))
            .bind(slug)
            .fetch_one(pool)
            .await
            .unwrap();

    let password_hash = hash_password("Passw0rd!").unwrap();
    let user_id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO users (organization_id, first_name, last_name, email, status, password_hash)
        VALUES ($1, 'Ada', 'Lovelace', $2, 'active', $3)
        RETURNING id
        "#,
    )
    .bind(org_id)
    .bind(email)
    .bind(&password_hash)
    .fetch_one(pool)
    .await
    .unwrap();

    let role_id: Uuid = sqlx::query_scalar("SELECT id FROM roles WHERE code = $1")
        .bind(role_code)
        .fetch_one(pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO user_roles (organization_id, user_id, role_id) VALUES ($1, $2, $3)")
        .bind(org_id)
        .bind(user_id)
        .bind(role_id)
        .execute(pool)
        .await
        .unwrap();

    let token = generate_token();
    let session_id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO sessions (user_id, organization_id, token_hash, expires_at)
        VALUES ($1, $2, $3, now() + interval '1 day')
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(org_id)
    .bind(hash_token(&token))
    .fetch_one(pool)
    .await
    .unwrap();

    Tenant {
        org_id,
        user_id,
        session_id,
        token,
    }
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Option<String>, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("cookie", format!("eat_session={token}"));
    }
    let request = match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };

    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap_or_default().to_string());
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, cookie, json)
}

#[sqlx::test(migrations = "./migrations")]
async fn login_logout_and_session_listing(pool: PgPool) {
    let app = router(&pool);
    let t = seed_tenant(&pool, "acme", "admin@acme.test", "admin").await;

    // Wrong password is rejected.
    let (status, _, _) = call(
        &app,
        "POST",
        "/v1/api/auth/login",
        None,
        Some(
            json!({ "organization_slug": "acme", "email": "admin@acme.test", "password": "nope" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Correct credentials issue a session cookie.
    let (status, cookie, body) = call(
        &app,
        "POST",
        "/v1/api/auth/login",
        None,
        Some(json!({ "organization_slug": "acme", "email": "admin@acme.test", "password": "Passw0rd!" })),
    ).await;
    assert_eq!(status, StatusCode::OK);
    let cookie = cookie.expect("session cookie");
    assert!(cookie.starts_with("eat_session="));
    assert_eq!(body["data"]["user"]["organization_id"], json!(t.org_id));
    assert!(
        body["data"]["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "invitations.create")
    );

    let login_token = cookie.trim_start_matches("eat_session=").to_string();
    let (status, _, me) = call(&app, "GET", "/v1/api/auth/me", Some(&login_token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["data"]["organization_id"], json!(t.org_id));

    // Two live sessions: the seeded one and the new login.
    let (status, _, sessions) =
        call(&app, "GET", "/v1/api/sessions", Some(&login_token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sessions["data"].as_array().unwrap().len(), 2);
    assert_eq!(
        sessions["data"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["current"] == true)
            .count(),
        1
    );

    // Logout revokes the current session.
    let (status, cleared, _) = call(
        &app,
        "POST",
        "/v1/api/auth/logout",
        Some(&login_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(cleared.unwrap().starts_with("eat_session="));
    let (status, _, _) = call(&app, "GET", "/v1/api/auth/me", Some(&login_token), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Revoke-all kills the remaining session too.
    let (status, _, _) = call(&app, "DELETE", "/v1/api/sessions", Some(&t.token), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = call(&app, "GET", "/v1/api/auth/me", Some(&t.token), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE organization_id = $1 AND action IN ('auth.login', 'auth.login_failed', 'auth.logout', 'session.revoked_all')",
    )
    .bind(t.org_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(events, 4, "failed login + login + logout + revoke-all");
}

#[sqlx::test(migrations = "./migrations")]
async fn invite_is_single_use_and_assigns_role(pool: PgPool) {
    let app = router(&pool);
    let admin = seed_tenant(&pool, "acme", "admin@acme.test", "admin").await;
    let viewer_role: Uuid = sqlx::query_scalar("SELECT id FROM roles WHERE code = 'viewer'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/api/invitations",
        Some(&admin.token),
        Some(json!({ "email": "New.User@Acme.test", "role_id": viewer_role, "expires_in_hours": 24 })),
    ).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let link = created["data"]["invite_link"].as_str().unwrap().to_string();
    let invitee_token = link.split("/invite/").nth(1).unwrap().to_string();
    assert_eq!(created["data"]["email"], "new.user@acme.test");

    // Acceptance page is public and shows the organization + role.
    let (status, _, preview) = call(
        &app,
        "GET",
        &format!("/v1/api/invite/{invitee_token}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["data"]["organization_name"], "Org acme");
    assert_eq!(preview["data"]["role_name"], "Viewer");
    assert_eq!(preview["data"]["status"], "pending");

    // Only hashes are stored.
    let stored: String =
        sqlx::query_scalar("SELECT token_hash FROM invitations WHERE organization_id = $1")
            .bind(admin.org_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, hash_token(&invitee_token));
    assert!(!stored.contains(&invitee_token));

    // Accept: profile is set, user becomes active, role applied.
    let (status, _, accepted) = call(
        &app,
        "POST",
        "/v1/api/invitations/accept",
        None,
        Some(json!({
            "token": invitee_token,
            "password": "Str0ngPass!",
            "first_name": "Grace",
            "last_name": "Hopper",
            "phone_number": "+213555000111",
            "avatar_url": "https://cdn.test/grace.png",
            "preferred_locale": "fr"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert_eq!(accepted["data"]["status"], "active");
    assert_eq!(accepted["data"]["preferred_locale"], "fr");

    let role_codes: Vec<String> = sqlx::query_scalar(
        r#"SELECT r.code FROM roles r JOIN user_roles ur ON ur.role_id = r.id
           WHERE ur.organization_id = $1 AND ur.user_id = $2"#,
    )
    .bind(admin.org_id)
    .bind(
        accepted["data"]["id"]
            .as_str()
            .unwrap()
            .parse::<Uuid>()
            .unwrap(),
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(role_codes, vec!["viewer".to_string()]);

    // The invitee can log in.
    let (status, viewer_cookie, login) = call(
        &app,
        "POST",
        "/v1/api/auth/login",
        None,
        Some(json!({ "organization_slug": "acme", "email": "new.user@acme.test", "password": "Str0ngPass!" })),
    ).await;
    assert_eq!(status, StatusCode::OK, "{login}");

    // Single use: the same token cannot be replayed.
    let (status, _, _) = call(
        &app,
        "POST",
        "/v1/api/invitations/accept",
        None,
        Some(json!({ "token": invitee_token, "password": "An0therPass!", "first_name": "Grace", "last_name": "Hopper" })),
    ).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // A viewer session cannot invite.
    let viewer_token = viewer_cookie
        .expect("viewer cookie")
        .trim_start_matches("eat_session=")
        .to_string();
    let (status, _, _) = call(
        &app,
        "POST",
        "/v1/api/invitations",
        Some(&viewer_token),
        Some(json!({ "email": "x@y.z", "role_id": viewer_role })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let audit_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE organization_id = $1 AND action = 'invitation.accepted'",
    )
    .bind(admin.org_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit_count, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn expired_invite_is_rejected(pool: PgPool) {
    let app = router(&pool);
    let admin = seed_tenant(&pool, "acme", "admin@acme.test", "admin").await;
    let viewer_role: Uuid = sqlx::query_scalar("SELECT id FROM roles WHERE code = 'viewer'")
        .fetch_one(&pool)
        .await
        .unwrap();

    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/api/invitations",
        Some(&admin.token),
        Some(json!({ "email": "late@acme.test", "role_id": viewer_role })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let link = created["data"]["invite_link"].as_str().unwrap().to_string();
    let token = link.split("/invite/").nth(1).unwrap().to_string();

    sqlx::query("UPDATE invitations SET expires_at = now() - interval '1 minute' WHERE organization_id = $1")
        .bind(admin.org_id)
        .execute(&pool)
        .await
        .unwrap();

    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/api/invitations/accept",
        None,
        Some(json!({ "token": token, "password": "Str0ngPass!", "first_name": "Late", "last_name": "User" })),
    ).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let status_in_db: String =
        sqlx::query_scalar("SELECT status FROM invitations WHERE organization_id = $1")
            .bind(admin.org_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status_in_db, "expired");
}

#[sqlx::test(migrations = "./migrations")]
async fn tenant_isolation_blocks_and_audits_cross_tenant_access(pool: PgPool) {
    let app = router(&pool);
    let a = seed_tenant(&pool, "tenant-a", "admin@a.test", "admin").await;
    let b = seed_tenant(&pool, "tenant-b", "admin@b.test", "admin").await;

    // B cannot revoke A's session.
    let (status, _, _) = call(
        &app,
        "DELETE",
        &format!("/v1/api/sessions/{}", a.session_id),
        Some(&b.token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A's session still works.
    let (status, _, _) = call(&app, "GET", "/v1/api/auth/me", Some(&a.token), None).await;
    assert_eq!(status, StatusCode::OK);

    let denied: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE organization_id = $1 AND action = 'tenant.cross_access_denied'",
    )
    .bind(b.org_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(denied, 1);

    // Sessions are scoped per tenant: B only sees its own.
    let (status, _, sessions) = call(&app, "GET", "/v1/api/sessions", Some(&b.token), None).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = sessions["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![b.session_id.to_string().as_str()]);

    // Same email in two tenants resolves to the right tenant at login.
    let (status, _, body) = call(
        &app,
        "POST",
        "/v1/api/auth/login",
        None,
        Some(json!({ "organization_slug": "tenant-a", "email": "admin@a.test", "password": "Passw0rd!" })),
    ).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["user"]["organization_id"], json!(a.org_id));

    // Cross-tenant login attempt does not leak into tenant B.
    let (status, _, _) = call(
        &app,
        "POST",
        "/v1/api/auth/login",
        None,
        Some(json!({ "organization_slug": "tenant-b", "email": "admin@a.test", "password": "Passw0rd!" })),
    ).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Suspension kills live sessions.
    let (status, _, _) = call(
        &app,
        "PATCH",
        &format!("/v1/api/users/{}/status", b.user_id),
        Some(&a.token),
        Some(json!({ "status": "suspended" })),
    )
    .await;
    // A cannot touch B's users either.
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _, _) = call(
        &app,
        "PATCH",
        &format!("/v1/api/users/{}/status", a.user_id),
        Some(&a.token),
        Some(json!({ "status": "suspended" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT); // cannot suspend yourself
}

#[sqlx::test(migrations = "./migrations")]
async fn suspended_user_sessions_are_revoked(pool: PgPool) {
    let app = router(&pool);
    let admin = seed_tenant(&pool, "acme", "admin@acme.test", "admin").await;
    let member = seed_tenant(&pool, "other", "member@acme.test", "viewer").await;

    // Move the member into the admin's org to simulate the same tenant.
    sqlx::query("UPDATE users SET organization_id = $1 WHERE id = $2")
        .bind(admin.org_id)
        .bind(member.user_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE user_roles SET organization_id = $1 WHERE user_id = $2")
        .bind(admin.org_id)
        .bind(member.user_id)
        .execute(&pool)
        .await
        .unwrap();
    let token_hash = hash_token(&member.token);
    sqlx::query("UPDATE sessions SET organization_id = $1, token_hash = $2 WHERE user_id = $3")
        .bind(admin.org_id)
        .bind(&token_hash)
        .bind(member.user_id)
        .execute(&pool)
        .await
        .unwrap();

    let (status, _, _) = call(
        &app,
        "PATCH",
        &format!("/v1/api/users/{}/status", member.user_id),
        Some(&admin.token),
        Some(json!({ "status": "suspended" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _, _) = call(&app, "GET", "/v1/api/auth/me", Some(&member.token), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
