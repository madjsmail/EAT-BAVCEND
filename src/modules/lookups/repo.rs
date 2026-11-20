// Lookup data access. Every statement filters by `organization_id`.
//
// Localized names are aggregated from `<entity>_translations` in the same
// query, so a list is one round trip and the API can return `name` plus the
// full locale map.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::api_error::ApiError;
use crate::modules::common::{derive_code, derive_slug, next_available_code, normalize_code};
use crate::modules::i18n::Translations;
use crate::modules::lookups::dto::*;

type Result<T> = std::result::Result<T, ApiError>;

fn trimmed(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn name_en(translations: &Translations) -> String {
    translations
        .get("en")
        .or_else(|| translations.values().next())
        .cloned()
        .unwrap_or_default()
}

async fn replace_translations(
    conn: &mut sqlx::PgConnection,
    table: &str,
    fk_column: &str,
    id: Uuid,
    translations: &Translations,
) -> Result<()> {
    // `table` and `fk_column` are internal constants, never user input.
    sqlx::query(&format!("DELETE FROM {table} WHERE {fk_column} = $1"))
        .bind(id)
        .execute(&mut *conn)
        .await?;

    for (locale, name) in translations {
        sqlx::query(&format!(
            "INSERT INTO {table} ({fk_column}, locale, name) VALUES ($1, $2, $3)
             ON CONFLICT ({fk_column}, locale) DO UPDATE SET name = EXCLUDED.name"
        ))
        .bind(id)
        .bind(locale)
        .bind(name.trim())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

// ------------------------------------------------------------ categories ----

#[derive(Debug, FromRow)]
pub struct CategoryRow {
    pub id: Uuid,
    pub code: String,
    pub category_type: String,
    pub skip_checkout_confirmation_emails: bool,
    pub icon_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[sqlx(json)]
    pub translations: BTreeMap<String, String>,
}

const CATEGORY_SELECT: &str = r#"
    SELECT c.id, c.code, c.category_type, c.skip_checkout_confirmation_emails, c.icon_url,
           c.created_at, c.updated_at,
           COALESCE((SELECT jsonb_object_agg(t.locale, t.name)
                     FROM category_translations t WHERE t.category_id = c.id), '{}'::jsonb) AS translations
    FROM categories c
    WHERE c.organization_id = $1
"#;

pub async fn list_categories(
    pool: &PgPool,
    org: Uuid,
    q: Option<&str>,
) -> Result<Vec<CategoryRow>> {
    let sql = format!(
        "{CATEGORY_SELECT}
         AND ($2::text IS NULL OR EXISTS (
             SELECT 1 FROM category_translations s
             WHERE s.category_id = c.id AND s.name ILIKE '%' || $2 || '%'))
         ORDER BY c.created_at, c.id"
    );
    Ok(sqlx::query_as::<_, CategoryRow>(&sql)
        .bind(org)
        .bind(q)
        .fetch_all(pool)
        .await?)
}

pub async fn get_category(pool: &PgPool, org: Uuid, id: Uuid) -> Result<CategoryRow> {
    let sql = format!("{CATEGORY_SELECT} AND c.id = $2");
    sqlx::query_as::<_, CategoryRow>(&sql)
        .bind(org)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("Category"))
}

pub async fn create_category(
    pool: &PgPool,
    org: Uuid,
    req: &CreateCategoryRequest,
) -> Result<CategoryRow> {
    let mut tx = pool.begin().await?;
    let code = match trimmed(&req.code) {
        Some(code) => normalize_code(&code),
        None => {
            let base = derive_code(&name_en(&req.translations));
            next_available_code(&mut tx, "categories", org, &base).await?
        }
    };

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO categories
            (organization_id, code, category_type, skip_checkout_confirmation_emails, icon_url)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(org)
    .bind(&code)
    .bind(&req.category_type)
    .bind(req.skip_checkout_confirmation_emails)
    .bind(trimmed(&req.icon_url))
    .fetch_one(&mut *tx)
    .await?;

    replace_translations(
        &mut tx,
        "category_translations",
        "category_id",
        id,
        &req.translations,
    )
    .await?;
    tx.commit().await?;
    get_category(pool, org, id).await
}

pub async fn update_category(
    pool: &PgPool,
    org: Uuid,
    id: Uuid,
    req: &UpdateCategoryRequest,
) -> Result<CategoryRow> {
    let current = get_category(pool, org, id).await?;
    let mut tx = pool.begin().await?;

    if let Some(translations) = &req.translations {
        replace_translations(
            &mut tx,
            "category_translations",
            "category_id",
            id,
            translations,
        )
        .await?;
    }

    sqlx::query(
        "UPDATE categories
         SET code = $3, category_type = $4, skip_checkout_confirmation_emails = $5,
             icon_url = $6, updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(id)
    .bind(org)
    .bind(
        req.code
            .as_deref()
            .map(normalize_code)
            .unwrap_or(current.code),
    )
    .bind(req.category_type.clone().unwrap_or(current.category_type))
    .bind(
        req.skip_checkout_confirmation_emails
            .unwrap_or(current.skip_checkout_confirmation_emails),
    )
    .bind(req.icon_url.clone().unwrap_or(current.icon_url))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    get_category(pool, org, id).await
}

pub async fn delete_category(pool: &PgPool, org: Uuid, id: Uuid) -> Result<()> {
    let done = sqlx::query("DELETE FROM categories WHERE id = $1 AND organization_id = $2")
        .bind(id)
        .bind(org)
        .execute(pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound("Category"));
    }
    Ok(())
}

// --------------------------------------------------------- manufacturers ----

#[derive(Debug, FromRow)]
pub struct ManufacturerRow {
    pub id: Uuid,
    pub url: Option<String>,
    pub support_url: Option<String>,
    pub support_phone: Option<String>,
    pub support_email: Option<String>,
    pub notes: Option<String>,
    pub logo_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[sqlx(json)]
    pub translations: BTreeMap<String, String>,
}

const MANUFACTURER_SELECT: &str = r#"
    SELECT m.id, m.url, m.support_url, m.support_phone, m.support_email, m.notes, m.logo_url,
           m.created_at, m.updated_at,
           COALESCE((SELECT jsonb_object_agg(t.locale, t.name)
                     FROM manufacturer_translations t WHERE t.manufacturer_id = m.id), '{}'::jsonb) AS translations
    FROM manufacturers m
    WHERE m.organization_id = $1
"#;

pub async fn list_manufacturers(
    pool: &PgPool,
    org: Uuid,
    q: Option<&str>,
) -> Result<Vec<ManufacturerRow>> {
    let sql = format!(
        "{MANUFACTURER_SELECT}
         AND ($2::text IS NULL OR EXISTS (
             SELECT 1 FROM manufacturer_translations s
             WHERE s.manufacturer_id = m.id AND s.name ILIKE '%' || $2 || '%'))
         ORDER BY m.created_at, m.id"
    );
    Ok(sqlx::query_as::<_, ManufacturerRow>(&sql)
        .bind(org)
        .bind(q)
        .fetch_all(pool)
        .await?)
}

pub async fn get_manufacturer(pool: &PgPool, org: Uuid, id: Uuid) -> Result<ManufacturerRow> {
    let sql = format!("{MANUFACTURER_SELECT} AND m.id = $2");
    sqlx::query_as::<_, ManufacturerRow>(&sql)
        .bind(org)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("Manufacturer"))
}

pub async fn create_manufacturer(
    pool: &PgPool,
    org: Uuid,
    req: &CreateManufacturerRequest,
) -> Result<ManufacturerRow> {
    let mut tx = pool.begin().await?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO manufacturers
            (organization_id, url, support_url, support_phone, support_email, notes, logo_url)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(org)
    .bind(trimmed(&req.url))
    .bind(trimmed(&req.support_url))
    .bind(trimmed(&req.support_phone))
    .bind(trimmed(&req.support_email))
    .bind(trimmed(&req.notes))
    .bind(trimmed(&req.logo_url))
    .fetch_one(&mut *tx)
    .await?;

    replace_translations(
        &mut tx,
        "manufacturer_translations",
        "manufacturer_id",
        id,
        &req.translations,
    )
    .await?;
    tx.commit().await?;
    get_manufacturer(pool, org, id).await
}

pub async fn update_manufacturer(
    pool: &PgPool,
    org: Uuid,
    id: Uuid,
    req: &UpdateManufacturerRequest,
) -> Result<ManufacturerRow> {
    let current = get_manufacturer(pool, org, id).await?;
    let mut tx = pool.begin().await?;

    if let Some(translations) = &req.translations {
        replace_translations(
            &mut tx,
            "manufacturer_translations",
            "manufacturer_id",
            id,
            translations,
        )
        .await?;
    }

    sqlx::query(
        "UPDATE manufacturers
         SET url = $3, support_url = $4, support_phone = $5, support_email = $6,
             notes = $7, logo_url = $8, updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(id)
    .bind(org)
    .bind(req.url.clone().unwrap_or(current.url))
    .bind(req.support_url.clone().unwrap_or(current.support_url))
    .bind(req.support_phone.clone().unwrap_or(current.support_phone))
    .bind(req.support_email.clone().unwrap_or(current.support_email))
    .bind(req.notes.clone().unwrap_or(current.notes))
    .bind(req.logo_url.clone().unwrap_or(current.logo_url))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    get_manufacturer(pool, org, id).await
}

pub async fn delete_manufacturer(pool: &PgPool, org: Uuid, id: Uuid) -> Result<()> {
    let done = sqlx::query("DELETE FROM manufacturers WHERE id = $1 AND organization_id = $2")
        .bind(id)
        .bind(org)
        .execute(pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound("Manufacturer"));
    }
    Ok(())
}

// --------------------------------------------------------------- statuses ----

#[derive(Debug, FromRow)]
pub struct StatusRow {
    pub id: Uuid,
    pub code: String,
    pub status_type: String,
    pub color_code: String,
    pub show_in_nav: bool,
    pub notes: Option<String>,
    pub assets_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[sqlx(json)]
    pub translations: BTreeMap<String, String>,
}

const STATUS_SELECT: &str = r#"
    SELECT s.id, s.code, s.status_type, s.color_code, s.show_in_nav, s.notes,
           s.created_at, s.updated_at,
           (SELECT count(*) FROM assets a
            WHERE a.status_id = s.id AND a.organization_id = s.organization_id) AS assets_count,
           COALESCE((SELECT jsonb_object_agg(t.locale, t.name)
                     FROM status_translations t WHERE t.status_id = s.id), '{}'::jsonb) AS translations
    FROM statuses s
    WHERE s.organization_id = $1
"#;

pub async fn list_statuses(pool: &PgPool, org: Uuid, q: Option<&str>) -> Result<Vec<StatusRow>> {
    let sql = format!(
        "{STATUS_SELECT}
         AND ($2::text IS NULL OR EXISTS (
             SELECT 1 FROM status_translations st
             WHERE st.status_id = s.id AND st.name ILIKE '%' || $2 || '%'))
         ORDER BY s.created_at, s.id"
    );
    Ok(sqlx::query_as::<_, StatusRow>(&sql)
        .bind(org)
        .bind(q)
        .fetch_all(pool)
        .await?)
}

pub async fn get_status(pool: &PgPool, org: Uuid, id: Uuid) -> Result<StatusRow> {
    let sql = format!("{STATUS_SELECT} AND s.id = $2");
    sqlx::query_as::<_, StatusRow>(&sql)
        .bind(org)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("Status"))
}

/// Status used by custody transitions: preferred by `code`, else first of `status_type`.
pub async fn find_status_for_transition(
    conn: &mut sqlx::PgConnection,
    org: Uuid,
    code: &str,
    status_type: &str,
) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar(
        "SELECT id FROM statuses
         WHERE organization_id = $1 AND (code = $2 OR status_type = $3)
         ORDER BY (code = $2) DESC, created_at
         LIMIT 1",
    )
    .bind(org)
    .bind(code)
    .bind(status_type)
    .fetch_optional(&mut *conn)
    .await
}

pub async fn create_status(
    pool: &PgPool,
    org: Uuid,
    req: &CreateStatusRequest,
) -> Result<StatusRow> {
    let mut tx = pool.begin().await?;
    let code = match trimmed(&req.code) {
        Some(code) => code.to_ascii_lowercase(),
        None => {
            let base = derive_slug(&name_en(&req.translations));
            next_available_code(&mut tx, "statuses", org, &base).await?
        }
    };

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO statuses
            (organization_id, code, status_type, color_code, show_in_nav, notes)
         VALUES ($1, $2, $3, COALESCE($4, '#6B7280'), $5, $6) RETURNING id",
    )
    .bind(org)
    .bind(&code)
    .bind(&req.status_type)
    .bind(trimmed(&req.color_code))
    .bind(req.show_in_nav)
    .bind(trimmed(&req.notes))
    .fetch_one(&mut *tx)
    .await?;

    replace_translations(
        &mut tx,
        "status_translations",
        "status_id",
        id,
        &req.translations,
    )
    .await?;
    tx.commit().await?;
    get_status(pool, org, id).await
}

pub async fn update_status(
    pool: &PgPool,
    org: Uuid,
    id: Uuid,
    req: &UpdateStatusRequest,
) -> Result<StatusRow> {
    let current = get_status(pool, org, id).await?;
    let mut tx = pool.begin().await?;

    if let Some(translations) = &req.translations {
        replace_translations(
            &mut tx,
            "status_translations",
            "status_id",
            id,
            translations,
        )
        .await?;
    }

    sqlx::query(
        "UPDATE statuses
         SET status_type = $3, color_code = $4, show_in_nav = $5, notes = $6, updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(id)
    .bind(org)
    .bind(req.status_type.clone().unwrap_or(current.status_type))
    .bind(req.color_code.clone().unwrap_or(current.color_code))
    .bind(req.show_in_nav.unwrap_or(current.show_in_nav))
    .bind(req.notes.clone().unwrap_or(current.notes))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    get_status(pool, org, id).await
}

pub async fn delete_status(pool: &PgPool, org: Uuid, id: Uuid) -> Result<()> {
    let done = sqlx::query("DELETE FROM statuses WHERE id = $1 AND organization_id = $2")
        .bind(id)
        .bind(org)
        .execute(pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound("Status"));
    }
    Ok(())
}

// ------------------------------------------------------------ departments ----

#[derive(Debug, FromRow)]
pub struct DepartmentRow {
    pub id: Uuid,
    pub code: String,
    pub email: Option<String>,
    pub phone_number: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[sqlx(json)]
    pub translations: BTreeMap<String, String>,
}

const DEPARTMENT_SELECT: &str = r#"
    SELECT d.id, d.code, d.email, d.phone_number, d.created_at, d.updated_at,
           COALESCE((SELECT jsonb_object_agg(t.locale, t.name)
                     FROM department_translations t WHERE t.department_id = d.id), '{}'::jsonb) AS translations
    FROM departments d
    WHERE d.organization_id = $1
"#;

pub async fn list_departments(
    pool: &PgPool,
    org: Uuid,
    q: Option<&str>,
) -> Result<Vec<DepartmentRow>> {
    let sql = format!(
        "{DEPARTMENT_SELECT}
         AND ($2::text IS NULL OR d.code ILIKE '%' || $2 || '%' OR EXISTS (
             SELECT 1 FROM department_translations s
             WHERE s.department_id = d.id AND s.name ILIKE '%' || $2 || '%'))
         ORDER BY d.code"
    );
    Ok(sqlx::query_as::<_, DepartmentRow>(&sql)
        .bind(org)
        .bind(q)
        .fetch_all(pool)
        .await?)
}

pub async fn get_department(pool: &PgPool, org: Uuid, id: Uuid) -> Result<DepartmentRow> {
    let sql = format!("{DEPARTMENT_SELECT} AND d.id = $2");
    sqlx::query_as::<_, DepartmentRow>(&sql)
        .bind(org)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("Department"))
}

pub async fn create_department(
    pool: &PgPool,
    org: Uuid,
    req: &CreateDepartmentRequest,
) -> Result<DepartmentRow> {
    let mut tx = pool.begin().await?;
    let code = match trimmed(&req.code) {
        Some(code) => normalize_code(&code),
        None => {
            let base = derive_code(&name_en(&req.translations));
            next_available_code(&mut tx, "departments", org, &base).await?
        }
    };

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO departments (organization_id, code, email, phone_number)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(org)
    .bind(&code)
    .bind(trimmed(&req.email))
    .bind(trimmed(&req.phone_number))
    .fetch_one(&mut *tx)
    .await?;

    replace_translations(
        &mut tx,
        "department_translations",
        "department_id",
        id,
        &req.translations,
    )
    .await?;
    tx.commit().await?;
    get_department(pool, org, id).await
}

pub async fn update_department(
    pool: &PgPool,
    org: Uuid,
    id: Uuid,
    req: &UpdateDepartmentRequest,
) -> Result<DepartmentRow> {
    let current = get_department(pool, org, id).await?;
    let mut tx = pool.begin().await?;

    if let Some(translations) = &req.translations {
        replace_translations(
            &mut tx,
            "department_translations",
            "department_id",
            id,
            translations,
        )
        .await?;
    }

    sqlx::query(
        "UPDATE departments
         SET code = $3, email = $4, phone_number = $5, updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(id)
    .bind(org)
    .bind(
        req.code
            .as_deref()
            .map(normalize_code)
            .unwrap_or(current.code),
    )
    .bind(req.email.clone().unwrap_or(current.email))
    .bind(req.phone_number.clone().unwrap_or(current.phone_number))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    get_department(pool, org, id).await
}

pub async fn delete_department(pool: &PgPool, org: Uuid, id: Uuid) -> Result<()> {
    let done = sqlx::query("DELETE FROM departments WHERE id = $1 AND organization_id = $2")
        .bind(id)
        .bind(org)
        .execute(pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound("Department"));
    }
    Ok(())
}

// -------------------------------------------------------------- locations ----

#[derive(Debug, FromRow)]
pub struct LocationRow {
    pub id: Uuid,
    pub code: String,
    pub parent_id: Option<Uuid>,
    pub location_type: String,
    pub address_line1: Option<String>,
    pub address_line2: Option<String>,
    pub city: Option<String>,
    pub state_region: Option<String>,
    pub postal_code: Option<String>,
    pub country_code: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub phone_number: Option<String>,
    pub email: Option<String>,
    pub manager_user_id: Option<Uuid>,
    pub notes: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[sqlx(json)]
    pub translations: BTreeMap<String, String>,
}

const LOCATION_SELECT: &str = r#"
    SELECT l.id, l.code, l.parent_id, l.location_type, l.address_line1, l.address_line2, l.city,
           l.state_region, l.postal_code, l.country_code, l.latitude, l.longitude,
           l.phone_number, l.email, l.manager_user_id, l.notes, l.is_active,
           l.created_at, l.updated_at,
           COALESCE((SELECT jsonb_object_agg(t.locale, t.name)
                     FROM location_translations t WHERE t.location_id = l.id), '{}'::jsonb) AS translations
    FROM locations l
    WHERE l.organization_id = $1
"#;

pub async fn list_locations(pool: &PgPool, org: Uuid, q: Option<&str>) -> Result<Vec<LocationRow>> {
    let sql = format!(
        "{LOCATION_SELECT}
         AND ($2::text IS NULL OR l.code ILIKE '%' || $2 || '%' OR EXISTS (
             SELECT 1 FROM location_translations s
             WHERE s.location_id = l.id AND s.name ILIKE '%' || $2 || '%'))
         ORDER BY l.code"
    );
    Ok(sqlx::query_as::<_, LocationRow>(&sql)
        .bind(org)
        .bind(q)
        .fetch_all(pool)
        .await?)
}

pub async fn get_location(pool: &PgPool, org: Uuid, id: Uuid) -> Result<LocationRow> {
    let sql = format!("{LOCATION_SELECT} AND l.id = $2");
    sqlx::query_as::<_, LocationRow>(&sql)
        .bind(org)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("Location"))
}

async fn check_location_parent(
    conn: &mut sqlx::PgConnection,
    org: Uuid,
    location_id: Option<Uuid>,
    parent_id: Uuid,
) -> Result<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM locations WHERE id = $1 AND organization_id = $2)",
    )
    .bind(parent_id)
    .bind(org)
    .fetch_one(&mut *conn)
    .await?;
    if !exists {
        return Err(ApiError::bad_request(
            "parent_id",
            "not_found",
            "Parent location does not exist",
        ));
    }

    let Some(location_id) = location_id else {
        return Ok(());
    };

    // Walk up from the requested parent; meeting ourselves means a cycle.
    let cycles: bool = sqlx::query_scalar(
        r#"
        WITH RECURSIVE ancestors AS (
            SELECT id, parent_id FROM locations WHERE id = $1 AND organization_id = $3
            UNION ALL
            SELECT l.id, l.parent_id
            FROM locations l JOIN ancestors a ON l.id = a.parent_id
        )
        SELECT EXISTS(SELECT 1 FROM ancestors WHERE id = $2)
        "#,
    )
    .bind(parent_id)
    .bind(location_id)
    .bind(org)
    .fetch_one(&mut *conn)
    .await?;
    if cycles {
        return Err(ApiError::bad_request(
            "parent_id",
            "cycle",
            "A location cannot be nested under its own descendant",
        ));
    }
    Ok(())
}

async fn check_manager(conn: &mut sqlx::PgConnection, org: Uuid, user_id: Uuid) -> Result<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM users WHERE id = $1 AND organization_id = $2)",
    )
    .bind(user_id)
    .bind(org)
    .fetch_one(&mut *conn)
    .await?;
    if !exists {
        return Err(ApiError::bad_request(
            "manager_user_id",
            "not_found",
            "Manager is not a user of this organization",
        ));
    }
    Ok(())
}

pub async fn create_location(
    pool: &PgPool,
    org: Uuid,
    req: &CreateLocationRequest,
) -> Result<LocationRow> {
    let mut tx = pool.begin().await?;
    if let Some(parent_id) = req.parent_id {
        check_location_parent(&mut tx, org, None, parent_id).await?;
    }
    if let Some(manager_id) = req.manager_user_id {
        check_manager(&mut tx, org, manager_id).await?;
    }

    let code = match trimmed(&req.code) {
        Some(code) => normalize_code(&code),
        None => {
            let base = derive_code(&name_en(&req.translations));
            next_available_code(&mut tx, "locations", org, &base).await?
        }
    };

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO locations
            (organization_id, code, parent_id, location_type, address_line1, address_line2, city,
             state_region, postal_code, country_code, latitude, longitude, phone_number, email,
             manager_user_id, notes, is_active)
         VALUES ($1, $2, $3, COALESCE($4, 'site'), $5, $6, $7, $8, $9, $10, $11, $12, $13,
                 $14, $15, $16, COALESCE($17, TRUE))
         RETURNING id",
    )
    .bind(org)
    .bind(&code)
    .bind(req.parent_id)
    .bind(req.location_type.as_deref())
    .bind(trimmed(&req.address_line1))
    .bind(trimmed(&req.address_line2))
    .bind(trimmed(&req.city))
    .bind(trimmed(&req.state_region))
    .bind(trimmed(&req.postal_code))
    .bind(trimmed(&req.country_code).map(|c| c.to_uppercase()))
    .bind(req.latitude)
    .bind(req.longitude)
    .bind(trimmed(&req.phone_number))
    .bind(trimmed(&req.email))
    .bind(req.manager_user_id)
    .bind(trimmed(&req.notes))
    .bind(req.is_active)
    .fetch_one(&mut *tx)
    .await?;

    replace_translations(
        &mut tx,
        "location_translations",
        "location_id",
        id,
        &req.translations,
    )
    .await?;
    tx.commit().await?;
    get_location(pool, org, id).await
}

pub async fn update_location(
    pool: &PgPool,
    org: Uuid,
    id: Uuid,
    req: &UpdateLocationRequest,
) -> Result<LocationRow> {
    let current = get_location(pool, org, id).await?;
    let mut tx = pool.begin().await?;

    if let Some(Some(parent_id)) = req.parent_id {
        check_location_parent(&mut tx, org, Some(id), parent_id).await?;
    }
    if let Some(Some(manager_id)) = req.manager_user_id {
        check_manager(&mut tx, org, manager_id).await?;
    }
    if let Some(translations) = &req.translations {
        replace_translations(
            &mut tx,
            "location_translations",
            "location_id",
            id,
            translations,
        )
        .await?;
    }

    sqlx::query(
        "UPDATE locations
         SET code = $3, parent_id = $4, location_type = $5, address_line1 = $6,
             address_line2 = $7, city = $8, state_region = $9, postal_code = $10,
             country_code = $11, latitude = $12, longitude = $13, phone_number = $14,
             email = $15, manager_user_id = $16, notes = $17, is_active = $18,
             updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(id)
    .bind(org)
    .bind(
        req.code
            .as_deref()
            .map(normalize_code)
            .unwrap_or(current.code),
    )
    .bind(req.parent_id.unwrap_or(current.parent_id))
    .bind(req.location_type.clone().unwrap_or(current.location_type))
    .bind(req.address_line1.clone().unwrap_or(current.address_line1))
    .bind(req.address_line2.clone().unwrap_or(current.address_line2))
    .bind(req.city.clone().unwrap_or(current.city))
    .bind(req.state_region.clone().unwrap_or(current.state_region))
    .bind(req.postal_code.clone().unwrap_or(current.postal_code))
    .bind(req.country_code.clone().unwrap_or(current.country_code))
    .bind(req.latitude.unwrap_or(current.latitude))
    .bind(req.longitude.unwrap_or(current.longitude))
    .bind(req.phone_number.clone().unwrap_or(current.phone_number))
    .bind(req.email.clone().unwrap_or(current.email))
    .bind(req.manager_user_id.unwrap_or(current.manager_user_id))
    .bind(req.notes.clone().unwrap_or(current.notes))
    .bind(req.is_active.unwrap_or(current.is_active))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    get_location(pool, org, id).await
}

pub async fn delete_location(pool: &PgPool, org: Uuid, id: Uuid) -> Result<()> {
    let done = sqlx::query("DELETE FROM locations WHERE id = $1 AND organization_id = $2")
        .bind(id)
        .bind(org)
        .execute(pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound("Location"));
    }
    Ok(())
}

// -------------------------------------------------------------- suppliers ----

#[derive(Debug, FromRow)]
pub struct SupplierRow {
    pub id: Uuid,
    pub contact_name: Option<String>,
    pub email: Option<String>,
    pub phone_number: Option<String>,
    pub address: Option<String>,
    pub url: Option<String>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[sqlx(json)]
    pub translations: BTreeMap<String, String>,
}

const SUPPLIER_SELECT: &str = r#"
    SELECT s.id, s.contact_name, s.email, s.phone_number, s.address, s.url, s.notes,
           s.created_at, s.updated_at,
           COALESCE((SELECT jsonb_object_agg(t.locale, t.name)
                     FROM supplier_translations t WHERE t.supplier_id = s.id), '{}'::jsonb) AS translations
    FROM suppliers s
    WHERE s.organization_id = $1
"#;

pub async fn list_suppliers(pool: &PgPool, org: Uuid, q: Option<&str>) -> Result<Vec<SupplierRow>> {
    let sql = format!(
        "{SUPPLIER_SELECT}
         AND ($2::text IS NULL OR EXISTS (
             SELECT 1 FROM supplier_translations st
             WHERE st.supplier_id = s.id AND st.name ILIKE '%' || $2 || '%'))
         ORDER BY s.created_at, s.id"
    );
    Ok(sqlx::query_as::<_, SupplierRow>(&sql)
        .bind(org)
        .bind(q)
        .fetch_all(pool)
        .await?)
}

pub async fn get_supplier(pool: &PgPool, org: Uuid, id: Uuid) -> Result<SupplierRow> {
    let sql = format!("{SUPPLIER_SELECT} AND s.id = $2");
    sqlx::query_as::<_, SupplierRow>(&sql)
        .bind(org)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("Supplier"))
}

pub async fn create_supplier(
    pool: &PgPool,
    org: Uuid,
    req: &CreateSupplierRequest,
) -> Result<SupplierRow> {
    let mut tx = pool.begin().await?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO suppliers
            (organization_id, contact_name, email, phone_number, address, url, notes)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(org)
    .bind(trimmed(&req.contact_name))
    .bind(trimmed(&req.email))
    .bind(trimmed(&req.phone_number))
    .bind(trimmed(&req.address))
    .bind(trimmed(&req.url))
    .bind(trimmed(&req.notes))
    .fetch_one(&mut *tx)
    .await?;

    replace_translations(
        &mut tx,
        "supplier_translations",
        "supplier_id",
        id,
        &req.translations,
    )
    .await?;
    tx.commit().await?;
    get_supplier(pool, org, id).await
}

pub async fn update_supplier(
    pool: &PgPool,
    org: Uuid,
    id: Uuid,
    req: &UpdateSupplierRequest,
) -> Result<SupplierRow> {
    let current = get_supplier(pool, org, id).await?;
    let mut tx = pool.begin().await?;

    if let Some(translations) = &req.translations {
        replace_translations(
            &mut tx,
            "supplier_translations",
            "supplier_id",
            id,
            translations,
        )
        .await?;
    }

    sqlx::query(
        "UPDATE suppliers
         SET contact_name = $3, email = $4, phone_number = $5, address = $6, url = $7,
             notes = $8, updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(id)
    .bind(org)
    .bind(req.contact_name.clone().unwrap_or(current.contact_name))
    .bind(req.email.clone().unwrap_or(current.email))
    .bind(req.phone_number.clone().unwrap_or(current.phone_number))
    .bind(req.address.clone().unwrap_or(current.address))
    .bind(req.url.clone().unwrap_or(current.url))
    .bind(req.notes.clone().unwrap_or(current.notes))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    get_supplier(pool, org, id).await
}

pub async fn delete_supplier(pool: &PgPool, org: Uuid, id: Uuid) -> Result<()> {
    let done = sqlx::query("DELETE FROM suppliers WHERE id = $1 AND organization_id = $2")
        .bind(id)
        .bind(org)
        .execute(pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound("Supplier"));
    }
    Ok(())
}

// --------------------------------------------------- depreciation models ----

#[derive(Debug, FromRow)]
pub struct DepreciationModelRow {
    pub id: Uuid,
    pub months: i32,
    pub floor_value: Decimal,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[sqlx(json)]
    pub translations: BTreeMap<String, String>,
}

const DEPRECIATION_SELECT: &str = r#"
    SELECT d.id, d.months, d.floor_value, d.notes, d.created_at, d.updated_at,
           COALESCE((SELECT jsonb_object_agg(t.locale, t.name)
                     FROM depreciation_model_translations t
                     WHERE t.depreciation_model_id = d.id), '{}'::jsonb) AS translations
    FROM depreciation_models d
    WHERE d.organization_id = $1
"#;

pub async fn list_depreciation_models(
    pool: &PgPool,
    org: Uuid,
    q: Option<&str>,
) -> Result<Vec<DepreciationModelRow>> {
    let sql = format!(
        "{DEPRECIATION_SELECT}
         AND ($2::text IS NULL OR EXISTS (
             SELECT 1 FROM depreciation_model_translations s
             WHERE s.depreciation_model_id = d.id AND s.name ILIKE '%' || $2 || '%'))
         ORDER BY d.months, d.id"
    );
    Ok(sqlx::query_as::<_, DepreciationModelRow>(&sql)
        .bind(org)
        .bind(q)
        .fetch_all(pool)
        .await?)
}

pub async fn get_depreciation_model(
    pool: &PgPool,
    org: Uuid,
    id: Uuid,
) -> Result<DepreciationModelRow> {
    let sql = format!("{DEPRECIATION_SELECT} AND d.id = $2");
    sqlx::query_as::<_, DepreciationModelRow>(&sql)
        .bind(org)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("Depreciation model"))
}

pub async fn create_depreciation_model(
    pool: &PgPool,
    org: Uuid,
    req: &CreateDepreciationModelRequest,
) -> Result<DepreciationModelRow> {
    let mut tx = pool.begin().await?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO depreciation_models (organization_id, months, floor_value, notes)
         VALUES ($1, $2, COALESCE($3, 0), $4) RETURNING id",
    )
    .bind(org)
    .bind(req.months)
    .bind(req.floor_value)
    .bind(trimmed(&req.notes))
    .fetch_one(&mut *tx)
    .await?;

    replace_translations(
        &mut tx,
        "depreciation_model_translations",
        "depreciation_model_id",
        id,
        &req.translations,
    )
    .await?;
    tx.commit().await?;
    get_depreciation_model(pool, org, id).await
}

pub async fn update_depreciation_model(
    pool: &PgPool,
    org: Uuid,
    id: Uuid,
    req: &UpdateDepreciationModelRequest,
) -> Result<DepreciationModelRow> {
    let current = get_depreciation_model(pool, org, id).await?;
    let mut tx = pool.begin().await?;

    if let Some(translations) = &req.translations {
        replace_translations(
            &mut tx,
            "depreciation_model_translations",
            "depreciation_model_id",
            id,
            translations,
        )
        .await?;
    }

    sqlx::query(
        "UPDATE depreciation_models
         SET months = $3, floor_value = $4, notes = $5, updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(id)
    .bind(org)
    .bind(req.months.unwrap_or(current.months))
    .bind(req.floor_value.unwrap_or(current.floor_value))
    .bind(req.notes.clone().unwrap_or(current.notes))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    get_depreciation_model(pool, org, id).await
}

pub async fn delete_depreciation_model(pool: &PgPool, org: Uuid, id: Uuid) -> Result<()> {
    let done =
        sqlx::query("DELETE FROM depreciation_models WHERE id = $1 AND organization_id = $2")
            .bind(id)
            .bind(org)
            .execute(pool)
            .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound("Depreciation model"));
    }
    Ok(())
}
