// Milestone 4 acceptance tests: the audit-log contract the web timeline relies on.

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
    .bind(hash_password("Passw0rd!").unwrap())
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

    Tenant {
        org_id,
        user_id,
        token,
    }
}

async fn call(app: &Router, uri: &str, token: Option<&str>) -> (StatusCode, Value) {
    let mut builder = Request::builder().method("GET").uri(uri);
    if let Some(token) = token {
        builder = builder.header("cookie", format!("eat_session={token}"));
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

struct AuditSeed<'a> {
    org: Uuid,
    actor: Option<Uuid>,
    action: &'a str,
    entity_type: &'a str,
    entity_id: Option<Uuid>,
    diff: Option<Value>,
    old_values: Option<Value>,
    new_values: Option<Value>,
    metadata: Value,
    minutes_ago: i32,
}

async fn insert_audit(pool: &PgPool, seed: AuditSeed<'_>) {
    sqlx::query(
        r#"
        INSERT INTO audit_logs (
            organization_id, actor_user_id, action, entity_type, entity_id,
            old_values, new_values, diff, metadata, created_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9,
                now() - make_interval(mins => $10))
        "#,
    )
    .bind(seed.org)
    .bind(seed.actor)
    .bind(seed.action)
    .bind(seed.entity_type)
    .bind(seed.entity_id)
    .bind(seed.old_values)
    .bind(seed.new_values)
    .bind(seed.diff)
    .bind(seed.metadata)
    .bind(seed.minutes_ago)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn audit_log_contract_matches_the_explorer(pool: PgPool) {
    let app = router(&pool);
    let t = seed_tenant(&pool, "acme", "admin@acme.test", "admin").await;
    let asset_id = Uuid::new_v4();
    // Seeding a tenant writes its own trigger audit rows; start from a clean feed.
    sqlx::query("DELETE FROM audit_logs")
        .execute(&pool)
        .await
        .unwrap();

    // UPDATE shape: diff carries both sides.
    insert_audit(
        &pool,
        AuditSeed {
            org: t.org_id,
            actor: Some(t.user_id),
            action: "updated",
            entity_type: "asset",
            entity_id: Some(asset_id),
            diff: Some(json!({ "priority": { "old": "high", "new": "critical" } })),
            old_values: Some(json!({ "priority": "high" })),
            new_values: Some(json!({ "priority": "critical" })),
            metadata: json!({ "source": "trigger" }),
            minutes_ago: 120,
        },
    )
    .await;

    // INSERT shape: only `new` is present, so `old` must come back as null.
    insert_audit(
        &pool,
        AuditSeed {
            org: t.org_id,
            actor: Some(t.user_id),
            action: "created",
            entity_type: "user",
            entity_id: Some(Uuid::new_v4()),
            diff: Some(json!({ "email": { "new": "new.hire@acme.test" } })),
            old_values: None,
            new_values: Some(json!({ "email": "new.hire@acme.test" })),
            metadata: json!({ "source": "trigger" }),
            minutes_ago: 60,
        },
    )
    .await;

    // Handler-written event: no diff at all, no actor.
    insert_audit(
        &pool,
        AuditSeed {
            org: t.org_id,
            actor: None,
            action: "login",
            entity_type: "user",
            entity_id: Some(t.user_id),
            diff: None,
            old_values: None,
            new_values: None,
            metadata: json!({ "email": "admin@acme.test" }),
            minutes_ago: 0,
        },
    )
    .await;

    // Another tenant must never leak into the timeline.
    let other_org: Uuid =
        sqlx::query_scalar("INSERT INTO organizations (name, slug) VALUES ('Other', 'other') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    insert_audit(
        &pool,
        AuditSeed {
            org: other_org,
            actor: None,
            action: "updated",
            entity_type: "asset",
            entity_id: None,
            diff: Some(json!({ "name": { "old": "a", "new": "b" } })),
            old_values: None,
            new_values: None,
            metadata: json!({}),
            minutes_ago: 0,
        },
    )
    .await;

    let (status, body) = call(&app, "/v1/api/audit-logs", Some(&t.token)).await;
    assert_eq!(status, StatusCode::OK, "list failed: {body}");
    assert_eq!(body["pagination"]["total"], 3);
    let rows = body["data"].as_array().unwrap();
    assert_eq!(rows.len(), 3);

    // Newest first: the no-diff login event.
    assert_eq!(rows[0]["event"], "login");
    assert_eq!(rows[0]["actor_name"], "System");
    assert_eq!(rows[0]["changes"], json!([]));
    assert!(rows[0]["diff"].is_null());

    // `changes` is always an array, even when `diff` is absent.
    assert_eq!(rows[1]["event"], "created");
    assert_eq!(
        rows[1]["changes"],
        json!([{ "field": "email", "old": null, "new": "new.hire@acme.test" }])
    );

    // UPDATE flattening plus server-side actor resolution.
    assert_eq!(rows[2]["event"], "updated");
    assert_eq!(rows[2]["actor_name"], "Ada Lovelace");
    assert_eq!(rows[2]["actor_email"], "admin@acme.test");
    assert_eq!(
        rows[2]["changes"],
        json!([{ "field": "priority", "old": "high", "new": "critical" }])
    );
    assert_eq!(
        rows[2]["diff"],
        json!({ "priority": { "old": "high", "new": "critical" } })
    );

    // Repeated `event` params.
    let (status, body) = call(
        &app,
        "/v1/api/audit-logs?event=login&event=updated",
        Some(&t.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["pagination"]["total"], 2);

    // Actor and entity filters.
    let (_, body) = call(
        &app,
        &format!("/v1/api/audit-logs?actor_id={}", t.user_id),
        Some(&t.token),
    )
    .await;
    assert_eq!(body["pagination"]["total"], 2);

    let (_, body) = call(
        &app,
        &format!("/v1/api/audit-logs?auditable_type=asset&auditable_id={asset_id}"),
        Some(&t.token),
    )
    .await;
    assert_eq!(body["pagination"]["total"], 1);

    // Free-text search covers diff/old/new/metadata, not actor names.
    let (_, body) = call(
        &app,
        "/v1/api/audit-logs?search=critical",
        Some(&t.token),
    )
    .await;
    assert_eq!(body["pagination"]["total"], 1);
    let (_, body) = call(&app, "/v1/api/audit-logs?search=Ada", Some(&t.token)).await;
    assert_eq!(body["pagination"]["total"], 0);

    // `per_page` is clamped to 100 rather than rejected.
    let (_, body) = call(
        &app,
        "/v1/api/audit-logs?per_page=500",
        Some(&t.token),
    )
    .await;
    assert_eq!(body["pagination"]["per_page"], 100);

    // Reversed range reports the field and the standard `invalid` code.
    let (status, body) = call(
        &app,
        "/v1/api/audit-logs?date_from=2026-11-21&date_to=2026-11-20",
        Some(&t.token),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error_stack"][0]["field"], "date_from");
    assert_eq!(body["error_stack"][0]["code"], "invalid");

    // Callers without `audit.read` are refused.
    let tech = seed_tenant(&pool, "techco", "tech@techco.test", "technician").await;
    let (status, body) = call(&app, "/v1/api/audit-logs", Some(&tech.token)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error_stack"][0]["code"], "forbidden");
}
