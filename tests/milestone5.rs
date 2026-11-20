// Milestone 5 acceptance tests: email-first auth, CSV import, offboarding.

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

async fn seed_tenant(pool: &PgPool, slug: &str) -> Tenant {
    let org_id: Uuid =
        sqlx::query_scalar("INSERT INTO organizations (name, slug) VALUES ($1, $2) RETURNING id")
            .bind(format!("Org {slug}"))
            .bind(slug)
            .fetch_one(pool)
            .await
            .unwrap();

    let user_id = insert_user(pool, org_id, "Ada", "Lovelace", "admin@acme.test", "active").await;
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

async fn insert_user(
    pool: &PgPool,
    org_id: Uuid,
    first: &str,
    last: &str,
    email: &str,
    status: &str,
) -> Uuid {
    let hash = hash_password("Passw0rd!").unwrap();
    sqlx::query_scalar(
        "INSERT INTO users (organization_id, first_name, last_name, email, status, password_hash)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(org_id)
    .bind(first)
    .bind(last)
    .bind(email)
    .bind(status)
    .bind(&hash)
    .fetch_one(pool)
    .await
    .unwrap()
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

async fn upload(
    app: &Router,
    token: &str,
    filename: &str,
    content_type: &str,
    body: &[u8],
) -> (StatusCode, Value) {
    const BOUNDARY: &str = "X-BOUNDARY-123";
    let mut payload = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"
    )
    .into_bytes();
    payload.extend_from_slice(body);
    payload.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    let request = Request::builder()
        .method("POST")
        .uri("/v1/api/users/import")
        .header("cookie", format!("eat_session={token}"))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(payload))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn upload_csv(app: &Router, token: &str, csv: &str) -> (StatusCode, Value) {
    upload(app, token, "users.csv", "text/csv", csv.as_bytes()).await
}

#[sqlx::test(migrations = "./migrations")]
async fn login_request_is_generic_and_setup_activates(pool: PgPool) {
    let app = router(&pool);
    let t = seed_tenant(&pool, "acme").await;

    let (status, _, unknown) = call(
        &app,
        "POST",
        "/v1/api/auth/login-request",
        None,
        Some(json!({ "organization_slug": "acme", "email": "nobody@acme.test" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let generic = unknown["message"].clone();

    // Admin creates an invited employee and gets a one-time setup link.
    let (status, _, created) = call(
        &app,
        "POST",
        "/v1/api/users",
        Some(&t.token),
        Some(json!({
            "email": "new.hire@acme.test",
            "first_name": "New",
            "last_name": "Hire",
            "preferred_locale": "fr"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create failed: {created}");
    assert_eq!(created["data"]["user"]["status"], "invited");
    let link = created["data"]["setup_link"].as_str().unwrap().to_string();
    let token = link.split("token=").nth(1).unwrap().to_string();

    // The setup link activates the account and starts a session.
    let (status, cookie, setup) = call(
        &app,
        "POST",
        "/v1/api/auth/setup-account",
        None,
        Some(json!({
            "token": token,
            "password": "Sup3rSecret!",
            "phone_number": "+216 20 000 000",
            "preferred_locale": "fr"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "setup failed: {setup}");
    assert!(cookie.unwrap().starts_with("eat_session="));
    assert_eq!(setup["data"]["user"]["status"], "active");
    assert_eq!(setup["data"]["user"]["preferred_locale"], "fr");

    // Now that the user is active, login-request still answers generically.
    let (status, _, known) = call(
        &app,
        "POST",
        "/v1/api/auth/login-request",
        None,
        Some(json!({ "organization_slug": "acme", "email": "new.hire@acme.test" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(known["message"], generic);

    let verified: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT email_verified_at FROM users WHERE organization_id = $1 AND email = 'new.hire@acme.test'")
            .bind(t.org_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(verified.is_some());

    // Password login works afterwards; the setup link is single-use.
    let (status, _, _) = call(
        &app,
        "POST",
        "/v1/api/auth/login",
        None,
        Some(json!({
            "organization_slug": "acme",
            "email": "new.hire@acme.test",
            "password": "Sup3rSecret!"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _, _) = call(
        &app,
        "POST",
        "/v1/api/auth/setup-account",
        None,
        Some(json!({ "token": token, "password": "AnotherPass1!" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn csv_import_creates_users_and_reports_bad_rows(pool: PgPool) {
    let app = router(&pool);
    let t = seed_tenant(&pool, "acme").await;

    let csv = "first_name,last_name,email,employee_id,job_title,department,location,manager_email,preferred_locale,phone_number\n\
        Yuki,Tanaka,yuki@acme.test,EMP-100,Engineer,Engineering,,,fr,\n\
        Omar,Fassi,omar@acme.test,EMP-101,Analyst,Finance,,,ar,\n\
        Yuki,Tanaka,yuki@acme.test,EMP-102,Duplicate,IT,,,en,\n\
        NoEmail,Person,,EMP-103,None,IT,,,en,\n";

    let (status, body) = upload_csv(&app, &t.token, csv).await;
    assert_eq!(status, StatusCode::OK, "import failed: {body}");
    assert_eq!(body["data"]["imported"], 2);
    assert_eq!(body["data"]["failed"], 2);
    assert_eq!(body["data"]["errors"].as_array().unwrap().len(), 2);

    let imported: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM users WHERE organization_id = $1 AND status = 'invited'",
    )
    .bind(t.org_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(imported, 2);

    // Missing department names are created on the fly.
    let engineering: Option<Uuid> = sqlx::query_scalar(
        "SELECT d.id FROM departments d
         JOIN department_translations t ON t.department_id = d.id
         WHERE d.organization_id = $1 AND t.name = 'Engineering'",
    )
    .bind(t.org_id)
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert!(engineering.is_some());

    // Imported employees get the base role.
    let role_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM user_roles ur JOIN roles r ON r.id = ur.role_id
         WHERE ur.organization_id = $1 AND r.code = 'employee'",
    )
    .bind(t.org_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(role_count, 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn xlsx_import_matches_csv_behaviour(pool: PgPool) {
    let app = router(&pool);
    let t = seed_tenant(&pool, "acme").await;

    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/users.xlsx"
    ))
    .unwrap();
    let (status, body) = upload(
        &app,
        &t.token,
        "users.xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        &bytes,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "xlsx import failed: {body}");
    assert_eq!(body["data"]["total_rows"], 3);
    assert_eq!(body["data"]["imported"], 2);
    assert_eq!(body["data"]["failed"], 1);

    // The spreadsheet row number (header = 1) is reported, not a flat index.
    let errors = body["data"]["errors"].as_array().unwrap();
    assert_eq!(errors[0]["row"], 4);
    assert_eq!(errors[0]["error"], "email is required");

    let locales: Vec<String> = sqlx::query_scalar(
        "SELECT preferred_locale FROM users WHERE organization_id = $1 AND status = 'invited' ORDER BY email",
    )
    .bind(t.org_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(locales, vec!["ar".to_string(), "fr".to_string()]);
}

#[sqlx::test(migrations = "./migrations")]
async fn offboarding_flags_returns_and_reassigns_reports(pool: PgPool) {
    let app = router(&pool);
    let t = seed_tenant(&pool, "acme").await;

    let manager = insert_user(&pool, t.org_id, "Grace", "Hopper", "grace@acme.test", "active").await;
    let report = insert_user(&pool, t.org_id, "Linus", "Torvalds", "linus@acme.test", "active").await;
    sqlx::query("UPDATE users SET manager_id = $1 WHERE id = $2")
        .bind(manager)
        .bind(report)
        .execute(&pool)
        .await
        .unwrap();

    // Minimal lookups + one asset held by the manager.
    let dept: Uuid = sqlx::query_scalar(
        "INSERT INTO departments (organization_id, code) VALUES ($1, 'IT') RETURNING id",
    )
    .bind(t.org_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let category: Uuid = sqlx::query_scalar(
        "INSERT INTO categories (organization_id, code, category_type) VALUES ($1, 'AST', 'asset') RETURNING id",
    )
    .bind(t.org_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let status: Uuid = sqlx::query_scalar(
        "INSERT INTO statuses (organization_id, code, status_type) VALUES ($1, 'deployed', 'deployed') RETURNING id",
    )
    .bind(t.org_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let product: Uuid = sqlx::query_scalar(
        "INSERT INTO products (organization_id, code, category_id) VALUES ($1, 'LAP', $2) RETURNING id",
    )
    .bind(t.org_id)
    .bind(category)
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO product_translations (product_id, locale, name) VALUES ($1, 'en', 'Laptop')",
    )
    .bind(product)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO assets (organization_id, asset_tag, product_id, status_id, department_id, name, assigned_user_id)
         VALUES ($1, 'LAP-0001', $2, $3, $4, 'Laptop', $5)",
    )
    .bind(t.org_id)
    .bind(product)
    .bind(status)
    .bind(dept)
    .bind(manager)
    .execute(&pool)
    .await
    .unwrap();

    // An accessory assignment with a partial return, and one live license seat.
    let accessory: Uuid = sqlx::query_scalar(
        "INSERT INTO accessories (organization_id, total_quantity, available_quantity)
         VALUES ($1, 3, 1) RETURNING id",
    )
    .bind(t.org_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO accessory_translations (accessory_id, locale, name) VALUES ($1, 'en', 'Headset')",
    )
    .bind(accessory)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO accessory_assignments (organization_id, accessory_id, assigned_to_user_id, quantity, returned_quantity)
         VALUES ($1, $2, $3, 2, 1)",
    )
    .bind(t.org_id)
    .bind(accessory)
    .bind(manager)
    .execute(&pool)
    .await
    .unwrap();
    let license: Uuid = sqlx::query_scalar(
        "INSERT INTO licenses (organization_id, total_seats) VALUES ($1, 5) RETURNING id",
    )
    .bind(t.org_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO license_translations (license_id, locale, name) VALUES ($1, 'en', 'Suite')",
    )
    .bind(license)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO license_seats (organization_id, license_id, user_id) VALUES ($1, $2, $3)")
        .bind(t.org_id)
        .bind(license)
        .bind(manager)
        .execute(&pool)
        .await
        .unwrap();

    // A manager with reports must name a replacement.
    let (status, _, _) = call(
        &app,
        "POST",
        &format!("/v1/api/users/{manager}/offboard"),
        Some(&t.token),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _, body) = call(
        &app,
        "POST",
        &format!("/v1/api/users/{manager}/offboard"),
        Some(&t.token),
        Some(json!({ "new_manager_id": t.user_id, "notes": "Left the company" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "offboard failed: {body}");
    assert_eq!(body["data"]["reassigned_reports"], 1);
    assert_eq!(body["data"]["return_requests_created"], 3);
    assert_eq!(body["data"]["pending_returns"].as_array().unwrap().len(), 3);
    assert_eq!(body["data"]["user"]["status"], "offboarded");

    let report_manager: Option<Uuid> = sqlx::query_scalar("SELECT manager_id FROM users WHERE id = $1")
        .bind(report)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(report_manager, Some(t.user_id));

    // The offboarded user can no longer sign in.
    let (status, _, _) = call(
        &app,
        "POST",
        "/v1/api/auth/login",
        None,
        Some(json!({
            "organization_slug": "acme",
            "email": "grace@acme.test",
            "password": "Passw0rd!"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
