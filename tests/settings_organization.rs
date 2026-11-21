// Milestone 5 (settings): self-service profile, password, sessions and
// organization settings.

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

const PASSWORD: &str = "Passw0rd!";

struct Tenant {
    org_id: Uuid,
    user_id: Uuid,
    token: String,
}

fn router(pool: &PgPool) -> Router {
    app(Arc::new(Database::from_pool(pool.clone())))
}

async fn seed_tenant(pool: &PgPool, slug: &str, email: &str, role_code: &str) -> Tenant {
    let org_id: Uuid =
        sqlx::query_scalar("INSERT INTO organizations (name, slug) VALUES ($1, $2) RETURNING id")
            .bind(format!("Org {slug}"))
            .bind(slug)
            .fetch_one(pool)
            .await
            .unwrap();

    let user_id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO users (organization_id, first_name, last_name, email, status, password_hash)
        VALUES ($1, 'Ada', 'Lovelace', $2, 'active', $3)
        RETURNING id
        "#,
    )
    .bind(org_id)
    .bind(email)
    .bind(hash_password(PASSWORD).unwrap())
    .fetch_one(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO user_roles (organization_id, user_id, role_id)
         SELECT $1, $2, id FROM roles WHERE code = $3",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(role_code)
    .execute(pool)
    .await
    .unwrap();

    let token = add_session(pool, org_id, user_id).await;
    Tenant {
        org_id,
        user_id,
        token,
    }
}

async fn add_session(pool: &PgPool, org_id: Uuid, user_id: Uuid) -> String {
    let token = generate_token();
    sqlx::query(
        "INSERT INTO sessions (user_id, organization_id, token_hash, expires_at)
         VALUES ($1, $2, $3, now() + interval '1 day')",
    )
    .bind(user_id)
    .bind(org_id)
    .bind(hash_token(&token))
    .execute(pool)
    .await
    .unwrap();
    token
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
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
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[sqlx::test(migrations = "./migrations")]
async fn profile_is_self_service(pool: PgPool) {
    let app = router(&pool);
    let t = seed_tenant(&pool, "acme", "admin@acme.test", "admin").await;

    let (status, _) = call(&app, "GET", "/v1/api/me", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = call(&app, "GET", "/v1/api/me", Some(&t.token), None).await;
    assert_eq!(status, StatusCode::OK, "me failed: {body}");
    assert_eq!(body["data"]["email"], "admin@acme.test");
    assert_eq!(body["data"]["role"], "admin");
    assert_eq!(body["data"]["organization_id"], json!(t.org_id));
    assert_eq!(body["data"]["name"], "Ada Lovelace");

    // Email, status and tenant are not self-editable; unknown keys are ignored.
    let (status, body) = call(
        &app,
        "PATCH",
        "/v1/api/me",
        Some(&t.token),
        Some(json!({
            "first_name": " Grace ",
            "last_name": "Hopper",
            "preferred_locale": "fr",
            "email": "hacker@evil.test",
            "status": "suspended"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "patch failed: {body}");
    assert_eq!(body["data"]["name"], "Grace Hopper");
    assert_eq!(body["data"]["preferred_locale"], "fr");
    assert_eq!(body["data"]["email"], "admin@acme.test");

    let (status, body) = call(
        &app,
        "PATCH",
        "/v1/api/me",
        Some(&t.token),
        Some(json!({ "preferred_locale": "de" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error_stack"][0]["field"], "preferred_locale");

    let (status, body) = call(
        &app,
        "PATCH",
        "/v1/api/me",
        Some(&t.token),
        Some(json!({ "avatar_url": "not-a-url" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error_stack"][0]["field"], "avatar_url");
}

#[sqlx::test(migrations = "./migrations")]
async fn password_change_revokes_only_other_sessions(pool: PgPool) {
    let app = router(&pool);
    let t = seed_tenant(&pool, "acme", "admin@acme.test", "admin").await;
    let other_device = add_session(&pool, t.org_id, t.user_id).await;

    let (status, body) = call(
        &app,
        "POST",
        "/v1/api/me/password",
        Some(&t.token),
        Some(json!({ "current_password": "wrong", "new_password": "BrandNew123!" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error_stack"][0]["field"], "current_password");

    let (status, body) = call(
        &app,
        "POST",
        "/v1/api/me/password",
        Some(&t.token),
        Some(json!({ "current_password": PASSWORD, "new_password": "short" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error_stack"][0]["field"], "new_password");

    let (status, body) = call(
        &app,
        "POST",
        "/v1/api/me/password",
        Some(&t.token),
        Some(json!({ "current_password": PASSWORD, "new_password": "NewPassw0rd!" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "password change failed: {body}");

    // The session that changed the password survives; the other device does not.
    let (status, _) = call(&app, "GET", "/v1/api/me", Some(&t.token), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(&app, "GET", "/v1/api/me", Some(&other_device), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // The new password works and the old one is rejected.
    let (status, _) = call(
        &app,
        "POST",
        "/v1/api/auth/login",
        None,
        Some(json!({ "email": "admin@acme.test", "password": "NewPassw0rd!" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &app,
        "POST",
        "/v1/api/auth/login",
        None,
        Some(json!({ "email": "admin@acme.test", "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "./migrations")]
async fn sessions_list_and_revocation(pool: PgPool) {
    let app = router(&pool);
    let t = seed_tenant(&pool, "acme", "admin@acme.test", "admin").await;
    let second = add_session(&pool, t.org_id, t.user_id).await;
    let third = add_session(&pool, t.org_id, t.user_id).await;

    let (status, body) = call(&app, "GET", "/v1/api/me/sessions", Some(&t.token), None).await;
    assert_eq!(status, StatusCode::OK);
    let sessions = body["data"].as_array().unwrap();
    assert_eq!(sessions.len(), 3);
    assert_eq!(
        sessions.iter().filter(|s| s["current"] == true).count(),
        1
    );
    assert_eq!(
        sessions.iter().filter(|s| s["is_current"] == true).count(),
        1
    );
    assert!(sessions.iter().all(|s| s["last_seen_at"].is_string()));

    // Revoke one specific device.
    let second_id: Uuid =
        sqlx::query_scalar("SELECT id FROM sessions WHERE token_hash = $1")
            .bind(hash_token(&second))
            .fetch_one(&pool)
            .await
            .unwrap();
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/v1/api/me/sessions/{second_id}"),
        Some(&t.token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(&app, "GET", "/v1/api/me", Some(&second), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Revoke every other device, keeping the current session.
    let (status, body) = call(
        &app,
        "DELETE",
        "/v1/api/me/sessions",
        Some(&t.token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "revoke all failed: {body}");
    assert_eq!(body["data"]["revoked"], 1);
    let (status, _) = call(&app, "GET", "/v1/api/me", Some(&third), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(&app, "GET", "/v1/api/me", Some(&t.token), None).await;
    assert_eq!(status, StatusCode::OK);

    // Someone else's session id is indistinguishable from a missing one.
    let stranger = seed_tenant(&pool, "other", "admin@other.test", "admin").await;
    let stranger_session: Uuid = sqlx::query_scalar("SELECT id FROM sessions WHERE user_id = $1")
        .bind(stranger.user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/v1/api/me/sessions/{stranger_session}"),
        Some(&t.token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn organization_settings_defaults_and_merge(pool: PgPool) {
    let app = router(&pool);
    let t = seed_tenant(&pool, "acme", "admin@acme.test", "admin").await;

    let (status, body) = call(&app, "GET", "/v1/api/organization", Some(&t.token), None).await;
    assert_eq!(status, StatusCode::OK, "get org failed: {body}");
    assert_eq!(body["data"]["default_locale"], "en");
    assert_eq!(body["data"]["settings"]["asset_tag_prefix"], "EAT-");
    assert_eq!(body["data"]["settings"]["asset_tag_padding"], 4);
    assert_eq!(body["data"]["settings"]["warranty_alert_days"], 30);
    assert_eq!(body["data"]["settings"]["license_alert_days"], 30);
    assert_eq!(body["data"]["settings"]["currency"], "USD");
    assert_eq!(body["data"]["settings"]["require_handover_signature"], false);
    assert!(body["data"]["logo_url"].is_null());

    let (status, body) = call(
        &app,
        "PATCH",
        "/v1/api/organization",
        Some(&t.token),
        Some(json!({
            "name": "Acme HQ",
            "default_locale": "fr",
            "settings": {
                "asset_tag_prefix": "ACME-",
                "asset_tag_padding": 6,
                "warranty_alert_days": 15,
                "currency": "eur",
                "require_handover_signature": true
            }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "patch org failed: {body}");
    assert_eq!(body["data"]["name"], "Acme HQ");
    assert_eq!(body["data"]["default_locale"], "fr");
    assert_eq!(body["data"]["settings"]["currency"], "EUR");
    assert_eq!(body["data"]["settings"]["asset_tag_padding"], 6);
    // Not supplied: keeps the previous value instead of resetting to default.
    assert_eq!(body["data"]["settings"]["license_alert_days"], 30);

    let (_, body) = call(&app, "GET", "/v1/api/organization", Some(&t.token), None).await;
    assert_eq!(body["data"]["settings"]["asset_tag_prefix"], "ACME-");
    assert_eq!(body["data"]["settings"]["require_handover_signature"], true);
}

#[sqlx::test(migrations = "./migrations")]
async fn organization_settings_validation_rbac_and_isolation(pool: PgPool) {
    let app = router(&pool);
    let admin = seed_tenant(&pool, "acme", "admin@acme.test", "admin").await;
    let tech = seed_tenant(&pool, "techco", "tech@techco.test", "technician").await;

    // Read is allowed, write is not.
    let (status, _) = call(&app, "GET", "/v1/api/organization", Some(&tech.token), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = call(
        &app,
        "PATCH",
        "/v1/api/organization",
        Some(&tech.token),
        Some(json!({ "name": "Hacked" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error_stack"][0]["code"], "forbidden");

    for (payload, field) in [
        (json!({ "settings": { "asset_tag_padding": 0 } }), "asset_tag_padding"),
        (json!({ "settings": { "asset_tag_padding": 11 } }), "asset_tag_padding"),
        (json!({ "settings": { "warranty_alert_days": -1 } }), "warranty_alert_days"),
        (json!({ "settings": { "currency": "US" } }), "currency"),
        (json!({ "settings": { "asset_tag_prefix": "   " } }), "asset_tag_prefix"),
        (json!({ "default_locale": "de" }), "default_locale"),
    ] {
        let (status, body) = call(
            &app,
            "PATCH",
            "/v1/api/organization",
            Some(&admin.token),
            Some(payload),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "for {field}");
        assert_eq!(body["error_stack"][0]["field"], field);
    }

    // Tenant isolation: the technician's org did not change.
    let (_, body) = call(&app, "GET", "/v1/api/organization", Some(&tech.token), None).await;
    assert_eq!(body["data"]["name"], "Org techco");
}

#[sqlx::test(migrations = "./migrations")]
async fn logo_upload_url_then_patch(pool: PgPool) {
    // Presigning is local but needs the Garage credentials from `.env`.
    let _ = dotenv::dotenv();
    let app = router(&pool);
    let t = seed_tenant(&pool, "acme", "admin@acme.test", "admin").await;

    let (status, body) = call(
        &app,
        "POST",
        "/v1/api/organization/logo/upload-url",
        Some(&t.token),
        Some(json!({ "file_name": "Logo Final.PNG", "content_type": "image/png" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "upload-url failed: {body}");
    let object_key = body["data"]["object_key"].as_str().unwrap().to_string();
    assert!(object_key.starts_with(&format!("{}/logo/{}/", t.org_id, t.org_id)));
    assert!(body["data"]["upload_url"].as_str().is_some());
    assert_eq!(body["data"]["expires_in_seconds"], 900);
    assert_eq!(body["data"]["max_bytes"], 2 * 1024 * 1024);

    let (status, body) = call(
        &app,
        "POST",
        "/v1/api/organization/logo/upload-url",
        Some(&t.token),
        Some(json!({ "file_name": "logo.svg", "content_type": "image/svg+xml" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error_stack"][0]["field"], "content_type");

    // Keys issued for another tenant are rejected.
    let stranger = seed_tenant(&pool, "other", "admin@other.test", "admin").await;
    let (status, body) = call(
        &app,
        "PATCH",
        "/v1/api/organization",
        Some(&t.token),
        Some(json!({ "logo_object_key": format!("{}/logo/{}/x.png", stranger.org_id, stranger.org_id) })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error_stack"][0]["field"], "logo_object_key");

    let (status, body) = call(
        &app,
        "PATCH",
        "/v1/api/organization",
        Some(&t.token),
        Some(json!({ "logo_object_key": object_key })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "logo patch failed: {body}");
    assert_eq!(body["data"]["logo_object_key"], object_key);
    if std::env::var("GARAGE_ACCESS_KEY_ID").is_ok_and(|v| !v.is_empty()) {
        let url = body["data"]["logo_url"].as_str().unwrap_or_default();
        assert!(url.contains("X-Amz-Signature"), "not presigned: {url}");
    }
}
