// CLI bootstrap: create the first organization and its Admin user.
//
//   cargo run -- bootstrap --org-name "ACME" --org-slug acme \
//       --email admin@acme.test --password 'S3curePass!' \
//       --first-name Ada --last-name Lovelace

use std::collections::HashMap;

use log::info;
use serde_json::json;

use crate::config::database::{Database, DatabaseTrait};
use crate::repository::{audit, organization, role, user};
use crate::service;

fn parse_args(args: Vec<String>) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();
    let mut iter = args.into_iter();
    while let Some(flag) = iter.next() {
        let key = flag
            .strip_prefix("--")
            .ok_or_else(|| format!("Unexpected argument: {flag}"))?;
        let value = iter
            .next()
            .ok_or_else(|| format!("Missing value for --{key}"))?;
        map.insert(key.to_string(), value);
    }
    Ok(map)
}

fn required(map: &HashMap<String, String>, key: &str) -> Result<String, String> {
    map.get(key)
        .cloned()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| format!("--{key} is required"))
}

pub async fn bootstrap(db: &Database, args: Vec<String>) -> Result<(), String> {
    let args = parse_args(args)?;

    let org_name = required(&args, "org-name")?;
    let org_slug = required(&args, "org-slug")?.to_lowercase();
    let email = required(&args, "email")?.to_lowercase();
    let password = required(&args, "password")?;
    let first_name = required(&args, "first-name")?;
    let last_name = required(&args, "last-name")?;

    if password.len() < 8 {
        return Err("--password must be at least 8 characters".to_string());
    }
    if !email.contains('@') {
        return Err("--email is not valid".to_string());
    }

    let pool = db.get_pool();

    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

    if organization::find_by_slug(pool, &org_slug)
        .await
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Err(format!("Organization slug '{org_slug}' already exists"));
    }

    let org = sqlx::query_as::<_, crate::entity::Organization>(
        "INSERT INTO organizations (name, slug) VALUES ($1, $2) RETURNING *",
    )
    .bind(&org_name)
    .bind(&org_slug)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    let admin_role = role::find_by_code(pool, "admin")
        .await
        .map_err(|e| e.to_string())?
        .ok_or("System role 'admin' is missing; run migrations")?;

    let hash = service::hash_password(&password)?;
    let admin = user::insert_active(
        &mut tx,
        org.id,
        &email,
        &first_name,
        &last_name,
        args.get("phone-number").map(String::as_str),
        None,
        "en",
        &hash,
    )
    .await
    .map_err(|e| e.to_string())?;

    role::assign(&mut tx, org.id, admin.id, admin_role.id, None)
        .await
        .map_err(|e| e.to_string())?;

    audit::insert_tx(
        &mut tx,
        audit::Entry {
            organization_id: Some(org.id),
            actor_user_id: Some(admin.id),
            action: "organization.bootstrapped",
            entity_type: "organization",
            entity_id: Some(org.id),
            metadata: json!({ "slug": org_slug, "admin_email": email }),
            ip_address: None,
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())?;

    info!("Bootstrapped organization {org_slug} with admin {email}");
    println!("organization_id={}", org.id);
    println!("organization_slug={}", org.slug);
    println!("admin_email={}", admin.email);
    Ok(())
}
