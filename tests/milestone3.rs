// Milestone 3 acceptance tests: accessory stock, consumable ledger + alerts,
// license seat limits + expiry windows, and the unified 360 view.

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

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = match body {
        Some(body) => Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", format!("eat_session={token}"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", format!("eat_session={token}"))
            .body(Body::empty())
            .unwrap(),
    };
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn find_by_translation(app: &Router, token: &str, path: &str, en: &str) -> Value {
    let (status, body) = call(app, "GET", path, token, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["data"]
        .as_array()
        .expect("expected an array")
        .iter()
        .find(|row| row["translations"]["en"] == en)
        .unwrap_or_else(|| panic!("{en} not found in {path}"))
        .clone()
}

async fn seeded_product(pool: &PgPool, org: Uuid, code: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM products WHERE organization_id = $1 AND code = $2")
        .bind(org)
        .bind(code)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn seeded_department(pool: &PgPool, org: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT id FROM departments WHERE organization_id = $1 AND code = 'COMM'")
        .bind(org)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn make_employee(pool: &PgPool, org: Uuid, email: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO users (organization_id, first_name, last_name, email, status, password_hash)
         VALUES ($1, 'Grace', 'Hopper', $2, 'active', $3) RETURNING id",
    )
    .bind(org)
    .bind(email)
    .bind(hash_password("Passw0rd!").unwrap())
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn accessory_assignment_moves_stock(pool: PgPool) {
    let app = router(&pool);
    let t = seed_tenant(&pool, "acme", "admin@acme.test").await;
    seed_defaults(&pool, "acme").await;

    let accessory =
        find_by_translation(&app, &t.token, "/v1/api/accessories", "USB-C Charger 65W").await;
    let accessory_id = accessory["id"].as_str().unwrap().to_string();
    assert_eq!(accessory["available_quantity"], 40);

    let employee = make_employee(&pool, t.org_id, "grace@acme.test").await;

    let (status, assigned) = call(
        &app,
        "POST",
        &format!("/v1/api/accessories/{accessory_id}/assign"),
        &t.token,
        Some(json!({ "user_id": employee, "quantity": 5, "notes": "onboarding kit" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{assigned}");
    assert_eq!(assigned["data"]["remaining_quantity"], 5);
    let assignment_id = assigned["data"]["id"].as_str().unwrap().to_string();

    let (_, accessory) = call(
        &app,
        "GET",
        &format!("/v1/api/accessories/{accessory_id}"),
        &t.token,
        None,
    )
    .await;
    assert_eq!(accessory["data"]["available_quantity"], 35);
    assert_eq!(accessory["data"]["assigned_quantity"], 5);

    // Partial return adds units back and keeps the row open.
    let (status, returned) = call(
        &app,
        "POST",
        &format!("/v1/api/accessories/{accessory_id}/assignments/{assignment_id}/return"),
        &t.token,
        Some(json!({ "quantity": 2 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{returned}");
    assert_eq!(returned["data"]["remaining_quantity"], 3);

    let (_, accessory) = call(
        &app,
        "GET",
        &format!("/v1/api/accessories/{accessory_id}"),
        &t.token,
        None,
    )
    .await;
    assert_eq!(accessory["data"]["available_quantity"], 37);

    // Returning more than outstanding is rejected.
    let (status, _) = call(
        &app,
        "POST",
        &format!("/v1/api/accessories/{accessory_id}/assignments/{assignment_id}/return"),
        &t.token,
        Some(json!({ "quantity": 10 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM audit_logs WHERE organization_id = $1 AND entity_id = $2 ORDER BY created_at",
    )
    .bind(t.org_id)
    .bind(Uuid::parse_str(&accessory_id).unwrap())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(actions.contains(&"assigned".to_string()));
    assert!(actions.contains(&"returned".to_string()));
}

#[sqlx::test(migrations = "./migrations")]
async fn consumable_ledger_is_atomic_and_flags_low_stock(pool: PgPool) {
    let app = router(&pool);
    let t = seed_tenant(&pool, "acme", "admin@acme.test").await;
    seed_defaults(&pool, "acme").await;

    let toner = find_by_translation(
        &app,
        &t.token,
        "/v1/api/consumables",
        "HP 26A Toner Cartridge",
    )
    .await;
    let toner_id = toner["id"].as_str().unwrap().to_string();
    assert_eq!(toner["quantity_on_hand"], 3);
    assert_eq!(toner["is_low_stock"], true);

    let (_, alerts) = call(&app, "GET", "/v1/api/consumables/alerts", &t.token, None).await;
    let names: Vec<&str> = alerts["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["translations"]["en"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"HP 26A Toner Cartridge"));
    assert!(names.contains(&"AA Batteries (4-pack)"));
    assert!(!names.contains(&"A4 Paper Ream"));

    let employee = make_employee(&pool, t.org_id, "grace@acme.test").await;

    let (status, issued) = call(
        &app,
        "POST",
        &format!("/v1/api/consumables/{toner_id}/transactions"),
        &t.token,
        Some(json!({ "transaction_type": "issue", "quantity": 2, "user_id": employee })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{issued}");

    let (_, consumable) = call(
        &app,
        "GET",
        &format!("/v1/api/consumables/{toner_id}"),
        &t.token,
        None,
    )
    .await;
    assert_eq!(consumable["data"]["quantity_on_hand"], 1);
    assert_eq!(consumable["data"]["is_low_stock"], true);

    // Over-issue must not drive stock negative.
    let (status, _) = call(
        &app,
        "POST",
        &format!("/v1/api/consumables/{toner_id}/transactions"),
        &t.token,
        Some(json!({ "transaction_type": "issue", "quantity": 5 })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, _) = call(
        &app,
        "POST",
        &format!("/v1/api/consumables/{toner_id}/transactions"),
        &t.token,
        Some(json!({ "transaction_type": "restock", "quantity": 10 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, consumable) = call(
        &app,
        "GET",
        &format!("/v1/api/consumables/{toner_id}"),
        &t.token,
        None,
    )
    .await;
    assert_eq!(consumable["data"]["quantity_on_hand"], 11);
    assert_eq!(consumable["data"]["is_low_stock"], false);

    let (_, transactions) = call(
        &app,
        "GET",
        &format!("/v1/api/consumables/{toner_id}/transactions"),
        &t.token,
        None,
    )
    .await;
    // Opening stock + issue + restock.
    assert_eq!(transactions["data"].as_array().unwrap().len(), 3);
}

#[sqlx::test(migrations = "./migrations")]
async fn license_seats_are_capped_and_expiry_windows_work(pool: PgPool) {
    let app = router(&pool);
    let t = seed_tenant(&pool, "acme", "admin@acme.test").await;
    seed_defaults(&pool, "acme").await;

    let employee = make_employee(&pool, t.org_id, "grace@acme.test").await;

    let (status, created) = call(
        &app,
        "POST",
        "/v1/api/licenses",
        &t.token,
        Some(json!({
            "translations": { "en": "Single Seat Tool", "fr": "Outil une place", "ar": "أداة مقعد واحد" },
            "total_seats": 1,
            "expiration_date": "2027-01-01"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let license_id = created["data"]["id"].as_str().unwrap().to_string();
    assert_eq!(created["data"]["available_seats"], 1);

    let (status, seat) = call(
        &app,
        "POST",
        &format!("/v1/api/licenses/{license_id}/seats/allocate"),
        &t.token,
        Some(json!({ "user_id": employee })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{seat}");

    let (status, over) = call(
        &app,
        "POST",
        &format!("/v1/api/licenses/{license_id}/seats/allocate"),
        &t.token,
        Some(json!({ "user_id": employee })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{over}");

    // Shrinking below active seats is rejected.
    let (status, _) = call(
        &app,
        "PATCH",
        &format!("/v1/api/licenses/{license_id}"),
        &t.token,
        Some(json!({ "total_seats": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let seat_id = seat["data"]["id"].as_str().unwrap().to_string();
    let (status, revoked) = call(
        &app,
        "POST",
        &format!("/v1/api/licenses/{license_id}/seats/{seat_id}/revoke"),
        &t.token,
        Some(json!({ "notes": "offboarded" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{revoked}");
    assert!(revoked["data"]["revoked_at"].is_string());

    let (_, alerts) = call(&app, "GET", "/v1/api/licenses/alerts", &t.token, None).await;
    let bucket = |key: &str| {
        alerts["data"][key]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|row| row["translations"]["en"].as_str())
            .collect::<Vec<_>>()
    };
    assert!(bucket("within_30_days").contains(&"Adobe Acrobat Pro"));
    assert!(bucket("within_60_days").contains(&"Zoom Workplace Pro"));
    assert!(bucket("within_90_days").contains(&"Slack Business+"));
    assert!(bucket("within_90_days").contains(&"JetBrains All Products Pack"));
    assert!(alerts["data"]["expired"].as_array().unwrap().is_empty());
    assert!(
        !bucket("within_90_days")
            .iter()
            .chain(bucket("within_60_days").iter())
            .any(|name| *name == "Microsoft 365 E3")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn asset_360_combines_all_pillars(pool: PgPool) {
    let app = router(&pool);
    let t = seed_tenant(&pool, "acme", "admin@acme.test").await;
    seed_defaults(&pool, "acme").await;

    let (status, asset) = call(
        &app,
        "POST",
        "/v1/api/assets",
        &t.token,
        Some(json!({
            "product_id": seeded_product(&pool, t.org_id, "LAP").await,
            "department_id": seeded_department(&pool, t.org_id).await,
            "name": "Finance laptop",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{asset}");
    let asset_id = asset["data"]["id"].as_str().unwrap().to_string();

    // Pair an accessory to the asset.
    let accessory =
        find_by_translation(&app, &t.token, "/v1/api/accessories", "USB-C Charger 65W").await;
    let (status, _) = call(
        &app,
        "POST",
        &format!(
            "/v1/api/accessories/{}/assign",
            accessory["id"].as_str().unwrap()
        ),
        &t.token,
        Some(json!({ "asset_id": asset_id, "quantity": 2 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Allocate a license seat to the asset itself.
    let m365 = find_by_translation(&app, &t.token, "/v1/api/licenses", "Microsoft 365 E3").await;
    let (status, _) = call(
        &app,
        "POST",
        &format!(
            "/v1/api/licenses/{}/seats/allocate",
            m365["id"].as_str().unwrap()
        ),
        &t.token,
        Some(json!({ "asset_id": asset_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, view) = call(
        &app,
        "GET",
        &format!("/v1/api/assets/{asset_id}/360"),
        &t.token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["data"]["counts"]["accessories"], 1);
    assert_eq!(view["data"]["counts"]["licenses"], 1);
    assert_eq!(view["data"]["counts"]["consumables"], 0);
    assert_eq!(view["data"]["accessories"][0]["asset_id"], asset_id);
    assert_eq!(view["data"]["licenses"][0]["license_id"], m365["id"]);

    // Things held by the asset's user show up too.
    let employee = make_employee(&pool, t.org_id, "grace@acme.test").await;
    let warehouse: Uuid = sqlx::query_scalar(
        "INSERT INTO locations (organization_id, code, location_type, city)
         VALUES ($1, 'WAREHOUSE', 'site', 'Tunis') RETURNING id",
    )
    .bind(t.org_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE users SET location_id = $1 WHERE id = $2")
        .bind(warehouse)
        .bind(employee)
        .execute(&pool)
        .await
        .unwrap();

    let (status, _) = call(
        &app,
        "POST",
        &format!("/v1/api/assets/{asset_id}/assign"),
        &t.token,
        Some(json!({ "user_id": employee })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let toner = find_by_translation(
        &app,
        &t.token,
        "/v1/api/consumables",
        "HP 26A Toner Cartridge",
    )
    .await;
    let (status, _) = call(
        &app,
        "POST",
        &format!(
            "/v1/api/consumables/{}/transactions",
            toner["id"].as_str().unwrap()
        ),
        &t.token,
        Some(json!({ "transaction_type": "issue", "quantity": 1, "user_id": employee })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let slack = find_by_translation(&app, &t.token, "/v1/api/licenses", "Slack Business+").await;
    let (status, _) = call(
        &app,
        "POST",
        &format!(
            "/v1/api/licenses/{}/seats/allocate",
            slack["id"].as_str().unwrap()
        ),
        &t.token,
        Some(json!({ "user_id": employee })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, view) = call(
        &app,
        "GET",
        &format!("/v1/api/assets/{asset_id}/360"),
        &t.token,
        None,
    )
    .await;
    assert_eq!(view["data"]["counts"]["consumables"], 1);
    assert_eq!(view["data"]["counts"]["licenses"], 2);
    assert_eq!(
        view["data"]["consumables"][0]["user_id"],
        employee.to_string()
    );
    assert_eq!(
        view["data"]["asset"]["assigned_user_id"],
        employee.to_string()
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn inventory_is_tenant_isolated(pool: PgPool) {
    let app = router(&pool);
    let acme = seed_tenant(&pool, "acme", "admin@acme.test").await;
    let globex = seed_tenant(&pool, "globex", "admin@globex.test").await;
    seed_defaults(&pool, "acme").await;
    seed_defaults(&pool, "globex").await;

    let accessory = find_by_translation(
        &app,
        &acme.token,
        "/v1/api/accessories",
        "USB-C Charger 65W",
    )
    .await;
    let license =
        find_by_translation(&app, &acme.token, "/v1/api/licenses", "Adobe Acrobat Pro").await;
    let consumable =
        find_by_translation(&app, &acme.token, "/v1/api/consumables", "A4 Paper Ream").await;

    for uri in [
        format!("/v1/api/accessories/{}", accessory["id"].as_str().unwrap()),
        format!("/v1/api/licenses/{}", license["id"].as_str().unwrap()),
        format!("/v1/api/consumables/{}", consumable["id"].as_str().unwrap()),
    ] {
        let (status, _) = call(&app, "GET", &uri, &globex.token, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
    }

    let (_, alerts) = call(
        &app,
        "GET",
        "/v1/api/consumables/alerts",
        &globex.token,
        None,
    )
    .await;
    let globex_names: Vec<&str> = alerts["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["translations"]["en"].as_str())
        .collect();
    // Globex has its own seeded copies, but never Acme's rows.
    assert!(globex_names.contains(&"HP 26A Toner Cartridge"));
    let _ = globex.user_id;
}
