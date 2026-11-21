// Milestone 6 acceptance test: schedule-driven, event-sourced maintenance.

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

async fn seed_tenant(pool: &PgPool, slug: &str, email: &str) -> Tenant {
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
         SELECT $1, $2, id FROM roles WHERE code = 'admin'",
    )
    .bind(org_id)
    .bind(user_id)
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

async fn seed_defaults(pool: &PgPool, slug: &str) {
    rust_api::seed::run(
        &Database::from_pool(pool.clone()),
        vec!["--org-slug".to_string(), slug.to_string()],
    )
    .await
    .expect("seed failed");
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
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[sqlx::test(migrations = "./migrations")]
async fn schedule_to_completed_work_order_timeline(pool: PgPool) {
    let app = router(&pool);
    let t = seed_tenant(&pool, "acme", "admin@acme.test").await;
    seed_defaults(&pool, "acme").await;

    let product: Uuid =
        sqlx::query_scalar("SELECT id FROM products WHERE organization_id = $1 AND code = 'LAP'")
            .bind(t.org_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let department: Uuid = sqlx::query_scalar(
        "SELECT id FROM departments WHERE organization_id = $1 AND code = 'COMM'",
    )
    .bind(t.org_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let (status, asset) = call(
        &app,
        "POST",
        "/v1/api/assets",
        Some(&t.token),
        Some(json!({
            "product_id": product,
            "department_id": department,
            "name": "Field laptop",
            "serial_number": "M6-001",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{asset}");
    let asset_id = asset["data"]["id"].as_str().unwrap().to_string();

    let (status, schedule) = call(
        &app,
        "POST",
        &format!("/v1/api/assets/{asset_id}/maintenance-schedules"),
        Some(&t.token),
        Some(json!({
            "frequency_type": "days",
            "interval": 30,
            "notes": "Monthly check",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{schedule}");
    let schedule_id = schedule["data"]["id"].as_str().unwrap().to_string();
    assert!(schedule["data"]["next_due_at"].is_string());

    // Make the schedule due, then run the cron sweep directly.
    sqlx::query(
        "UPDATE maintenance_schedules SET next_due_at = now() - interval '1 day'
         WHERE id = $1",
    )
    .bind(Uuid::parse_str(&schedule_id).unwrap())
    .execute(&pool)
    .await
    .unwrap();

    let summary = rust_api::modules::maintenances::run_once(&pool)
        .await
        .expect("scheduler run");
    assert_eq!(summary.generated, 1);
    assert_eq!(summary.errors, 0);

    // Idempotent: a second run must not create another open work order.
    let second = rust_api::modules::maintenances::run_once(&pool)
        .await
        .expect("second scheduler run");
    assert_eq!(second.generated, 0);

    let (status, board) = call(
        &app,
        "GET",
        &format!("/v1/api/work-orders?asset_id={asset_id}"),
        Some(&t.token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{board}");
    let rows = board["data"].as_array().unwrap();
    let work_order = rows
        .iter()
        .find(|row| row["type"] == "preventive")
        .expect("preventive work order");
    let work_order_id = work_order["id"].as_str().unwrap().to_string();
    assert!(work_order["due_date"].is_string());
    assert_eq!(work_order["status"], "open");

    let (status, updated) = call(
        &app,
        "PATCH",
        &format!("/v1/api/work-orders/{work_order_id}"),
        Some(&t.token),
        Some(json!({ "assigned_to": t.user_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["data"]["assigned_to"], t.user_id.to_string());

    let (status, updated) = call(
        &app,
        "PATCH",
        &format!("/v1/api/work-orders/{work_order_id}"),
        Some(&t.token),
        Some(json!({ "status": "in_progress" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["data"]["status"], "in_progress");

    let (status, completed) = call(
        &app,
        "PATCH",
        &format!("/v1/api/work-orders/{work_order_id}"),
        Some(&t.token),
        Some(json!({ "status": "done", "cost": 150.00 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{completed}");
    assert_eq!(completed["data"]["status"], "done");
    assert!(completed["data"]["completed_at"].is_string());

    let (status, history) = call(
        &app,
        "GET",
        &format!("/v1/api/assets/{asset_id}/maintenance-history"),
        Some(&t.token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{history}");
    let events = history["data"].as_array().unwrap();
    assert!(events.len() >= 4, "expected 4+ events, got {events:#?}");

    let kinds: Vec<&str> = events
        .iter()
        .map(|event| event["kind"].as_str().unwrap_or_default())
        .collect();
    for expected in [
        "work_order_created",
        "assigned",
        "status_changed",
        "completed",
    ] {
        assert!(kinds.contains(&expected), "missing {expected} in {kinds:?}");
    }

    // Chronologically descending.
    let timestamps: Vec<&str> = events
        .iter()
        .map(|event| event["occurred_at"].as_str().unwrap())
        .collect();
    let mut sorted = timestamps.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(timestamps, sorted);
}
