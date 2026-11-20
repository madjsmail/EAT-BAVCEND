// Milestone 2 acceptance tests: localized lookups, asset tags, quick-scan,
// barcode labels, custody history + audit trail, tenant isolation.

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

    let role_id: Uuid = sqlx::query_scalar("SELECT id FROM roles WHERE code = 'admin'")
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

async fn raw(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
    accept_language: Option<&str>,
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("cookie", format!("eat_session={token}"));
    }
    if let Some(language) = accept_language {
        builder = builder.header("accept-language", language);
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
    (status, bytes.to_vec())
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
    accept_language: Option<&str>,
) -> (StatusCode, Value) {
    let (status, bytes) = raw(app, method, uri, token, body, accept_language).await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn row_by_code<'a>(data: &'a Value, code: &str) -> &'a Value {
    data.as_array()
        .expect("expected an array")
        .iter()
        .find(|row| row["code"] == code)
        .expect("row not found")
}

#[sqlx::test(migrations = "./migrations")]
async fn seeder_populates_localized_lookups(pool: PgPool) {
    let app = router(&pool);
    let t = seed_tenant(&pool, "acme", "admin@acme.test").await;
    seed_defaults(&pool, "acme").await;

    let (status, body) = call(
        &app,
        "GET",
        "/v1/api/lookups/statuses",
        Some(&t.token),
        None,
        Some("fr"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let statuses = &body["data"];
    assert_eq!(statuses.as_array().unwrap().len(), 7);
    assert_eq!(row_by_code(statuses, "deployed")["name"], "Déployé");
    assert_eq!(row_by_code(statuses, "broken")["name"], "En panne");

    let (_, body) = call(
        &app,
        "GET",
        "/v1/api/lookups/statuses",
        Some(&t.token),
        None,
        Some("ar"),
    )
    .await;
    assert_eq!(row_by_code(&body["data"], "deployed")["name"], "موزع");
    assert_eq!(
        row_by_code(&body["data"], "ready_to_deploy")["translations"]["en"],
        "Ready to Deploy"
    );

    // No header: falls back to the user's preferred locale (`en`).
    let (_, body) = call(
        &app,
        "GET",
        "/v1/api/lookups/categories",
        Some(&t.token),
        None,
        None,
    )
    .await;
    assert_eq!(body["data"].as_array().unwrap().len(), 4);
    assert_eq!(row_by_code(&body["data"], "AST")["name"], "Asset");

    // Idempotent: running the seeder twice does not duplicate rows.
    seed_defaults(&pool, "acme").await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM statuses WHERE organization_id = $1")
        .bind(t.org_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 7);
}

async fn product_id(pool: &PgPool, org: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT id FROM products WHERE organization_id = $1 AND code = 'LAP'")
        .bind(org)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn department_id(pool: &PgPool, org: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT id FROM departments WHERE organization_id = $1 AND code = 'COMM'")
        .bind(org)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn asset_tags_are_sequential_and_scannable(pool: PgPool) {
    let app = router(&pool);
    let t = seed_tenant(&pool, "acme", "admin@acme.test").await;
    seed_defaults(&pool, "acme").await;

    let product = product_id(&pool, t.org_id).await;
    let department = department_id(&pool, t.org_id).await;
    let create = |name: &str, serial: &str| {
        json!({
            "product_id": product,
            "department_id": department,
            "name": name,
            "serial_number": serial,
        })
    };

    let (status, first) = call(
        &app,
        "POST",
        "/v1/api/assets",
        Some(&t.token),
        Some(create("Laptop one", "SN-001")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["data"]["asset_tag"], "COMM.AST.LAP.0001");

    let (_, second) = call(
        &app,
        "POST",
        "/v1/api/assets",
        Some(&t.token),
        Some(create("Laptop two", "SN-002")),
        None,
    )
    .await;
    assert_eq!(second["data"]["asset_tag"], "COMM.AST.LAP.0002");

    // Quick-scan resolves by asset tag and by serial number.
    let (status, scan) = call(
        &app,
        "GET",
        "/v1/api/assets/scan?code=COMM.AST.LAP.0002",
        Some(&t.token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(scan["data"]["id"], second["data"]["id"]);

    let (_, scan) = call(
        &app,
        "GET",
        "/v1/api/assets/scan?code=SN-001",
        Some(&t.token),
        None,
        None,
    )
    .await;
    assert_eq!(scan["data"]["id"], first["data"]["id"]);

    let asset_id = first["data"]["id"].as_str().unwrap();
    let (status, label) = call(
        &app,
        "GET",
        &format!("/v1/api/assets/{asset_id}/label?format=json"),
        Some(&t.token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(label["data"]["data"], "COMM.AST.LAP.0001");
    assert_eq!(label["data"]["symbology"], "qr");

    let (status, svg) = raw(
        &app,
        "GET",
        &format!("/v1/api/assets/{asset_id}/label?format=svg&symbology=code128"),
        Some(&t.token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&svg));
    assert!(String::from_utf8_lossy(&svg).starts_with("<svg"));

    let (status, png) = raw(
        &app,
        "GET",
        &format!("/v1/api/assets/{asset_id}/label?format=png"),
        Some(&t.token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}

#[sqlx::test(migrations = "./migrations")]
async fn custody_transitions_are_audited(pool: PgPool) {
    let app = router(&pool);
    let t = seed_tenant(&pool, "acme", "admin@acme.test").await;
    seed_defaults(&pool, "acme").await;

    let warehouse: Uuid = sqlx::query_scalar(
        "INSERT INTO locations (organization_id, code, location_type, city)
         VALUES ($1, 'WAREHOUSE', 'site', 'Tunis') RETURNING id",
    )
    .bind(t.org_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let home: Uuid = sqlx::query_scalar(
        "INSERT INTO locations (organization_id, code, location_type, city)
         VALUES ($1, 'HOME', 'site', 'Sfax') RETURNING id",
    )
    .bind(t.org_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let employee: Uuid = sqlx::query_scalar(
        "INSERT INTO users (organization_id, first_name, last_name, email, status, password_hash, location_id)
         VALUES ($1, 'Grace', 'Hopper', 'grace@acme.test', 'active', $2, $3) RETURNING id",
    )
    .bind(t.org_id)
    .bind(hash_password("Passw0rd!").unwrap())
    .bind(home)
    .fetch_one(&pool)
    .await
    .unwrap();

    let (_, asset) = call(
        &app,
        "POST",
        "/v1/api/assets",
        Some(&t.token),
        Some(json!({
            "product_id": product_id(&pool, t.org_id).await,
            "department_id": department_id(&pool, t.org_id).await,
            "name": "Custody laptop",
            "location_id": warehouse,
        })),
        None,
    )
    .await;
    let asset_id = asset["data"]["id"].as_str().unwrap().to_string();

    let (status, assigned) = call(
        &app,
        "POST",
        &format!("/v1/api/assets/{asset_id}/assign"),
        Some(&t.token),
        Some(json!({ "user_id": employee, "notes": "onboarding" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{assigned}");
    assert_eq!(assigned["data"]["assigned_user_id"], employee.to_string());
    // Assigning to a user inherits the user's location.
    assert_eq!(assigned["data"]["location_id"], home.to_string());
    assert_eq!(assigned["data"]["status_type"], "deployed");

    let (status, returned) = call(
        &app,
        "POST",
        &format!("/v1/api/assets/{asset_id}/return"),
        Some(&t.token),
        Some(json!({ "location_id": warehouse })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{returned}");
    assert!(returned["data"]["assigned_user_id"].is_null());
    assert_eq!(returned["data"]["status_type"], "deployable");

    let (status, transferred) = call(
        &app,
        "POST",
        &format!("/v1/api/assets/{asset_id}/transfer"),
        Some(&t.token),
        Some(json!({ "location_id": home, "notes": "site move" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{transferred}");
    assert_eq!(transferred["data"]["location_id"], home.to_string());

    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM audit_logs WHERE organization_id = $1 AND entity_id = $2 ORDER BY created_at",
    )
    .bind(t.org_id)
    .bind(Uuid::parse_str(&asset_id).unwrap())
    .fetch_all(&pool)
    .await
    .unwrap();
    for expected in ["created", "assigned", "returned", "transferred"] {
        assert!(actions.contains(&expected.to_string()), "missing {expected} in {actions:?}");
    }

    let history: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM asset_assignments WHERE organization_id = $1 AND asset_id = $2",
    )
    .bind(t.org_id)
    .bind(Uuid::parse_str(&asset_id).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(history, 3);
}

#[sqlx::test(migrations = "./migrations")]
async fn assets_are_tenant_isolated(pool: PgPool) {
    let app = router(&pool);
    let acme = seed_tenant(&pool, "acme", "admin@acme.test").await;
    let other = seed_tenant(&pool, "globex", "admin@globex.test").await;
    seed_defaults(&pool, "acme").await;
    seed_defaults(&pool, "globex").await;

    let (_, asset) = call(
        &app,
        "POST",
        "/v1/api/assets",
        Some(&acme.token),
        Some(json!({
            "product_id": product_id(&pool, acme.org_id).await,
            "department_id": department_id(&pool, acme.org_id).await,
            "name": "Acme laptop",
        })),
        None,
    )
    .await;
    let asset_id = asset["data"]["id"].as_str().unwrap().to_string();

    for uri in [
        format!("/v1/api/assets/{asset_id}"),
        format!("/v1/api/assets/{asset_id}/assignments"),
        format!("/v1/api/assets/{asset_id}/label?format=json"),
    ] {
        let (status, _) = call(&app, "GET", &uri, Some(&other.token), None, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
    }

    let (status, _) = call(
        &app,
        "GET",
        "/v1/api/assets/scan?code=COMM.AST.LAP.0001",
        Some(&other.token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
