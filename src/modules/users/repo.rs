// Data access for the employee directory, bulk import, and offboarding.
// Every query filters by organization_id first.

use chrono::DateTime;
use chrono::Utc;
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

use crate::entity::User;

type Result<T> = sqlx::Result<T>;

#[derive(Debug, FromRow)]
pub struct UserRow {
    pub id: Uuid,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub employee_id: Option<String>,
    pub job_title: Option<String>,
    pub department_id: Option<Uuid>,
    pub department_name: Option<String>,
    pub location_id: Option<Uuid>,
    pub location_name: Option<String>,
    pub manager_id: Option<Uuid>,
    pub manager_name: Option<String>,
    pub preferred_locale: String,
    pub status: String,
    pub avatar_url: Option<String>,
    pub phone_number: Option<String>,
    pub email_verified_at: Option<DateTime<Utc>>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

const USER_SELECT: &str = r#"
    SELECT u.id, u.email, u.first_name, u.last_name, u.employee_id, u.job_title,
           u.department_id, dt.name AS department_name,
           u.location_id, lt.name AS location_name,
           u.manager_id,
           NULLIF(btrim(m.first_name || ' ' || m.last_name), '') AS manager_name,
           u.preferred_locale, u.status, u.avatar_url, u.phone_number,
           u.email_verified_at, u.last_login_at, u.created_at
    FROM users u
    LEFT JOIN department_translations dt
           ON dt.department_id = u.department_id AND dt.locale = $2
    LEFT JOIN location_translations lt
           ON lt.location_id = u.location_id AND lt.locale = $2
    LEFT JOIN users m ON m.id = u.manager_id AND m.organization_id = u.organization_id
"#;

#[allow(clippy::too_many_arguments)]
pub async fn list_users(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    search: Option<&str>,
    department_id: Option<Uuid>,
    location_id: Option<Uuid>,
    status: Option<&str>,
    manager_id: Option<Uuid>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<UserRow>, i64)> {
    let filter = r#"
        WHERE u.organization_id = $1 AND u.deleted_at IS NULL
          AND ($3::text IS NULL
               OR u.first_name ILIKE '%' || $3 || '%'
               OR u.last_name ILIKE '%' || $3 || '%'
               OR u.email ILIKE '%' || $3 || '%'
               OR u.employee_id ILIKE '%' || $3 || '%'
               OR (u.first_name || ' ' || u.last_name) ILIKE '%' || $3 || '%')
          AND ($4::uuid IS NULL OR u.department_id = $4)
          AND ($5::uuid IS NULL OR u.location_id = $5)
          AND ($6::text IS NULL OR u.status = $6)
          AND ($7::uuid IS NULL OR u.manager_id = $7)
    "#;

    let sql = format!(
        "{USER_SELECT} {filter} ORDER BY u.last_name, u.first_name, u.id LIMIT $8 OFFSET $9"
    );
    let rows = sqlx::query_as::<_, UserRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(search)
        .bind(department_id)
        .bind(location_id)
        .bind(status)
        .bind(manager_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

    let count_sql = format!("SELECT count(*) FROM users u {filter}");
    let total: i64 = sqlx::query_scalar(&count_sql)
        .bind(org)
        .bind(locale)
        .bind(search)
        .bind(department_id)
        .bind(location_id)
        .bind(status)
        .bind(manager_id)
        .fetch_one(pool)
        .await?;

    Ok((rows, total))
}

pub async fn find_by_id(pool: &PgPool, org: Uuid, id: Uuid, locale: &str) -> Result<Option<UserRow>> {
    let sql = format!("{USER_SELECT} WHERE u.organization_id = $1 AND u.id = $3 AND u.deleted_at IS NULL");
    sqlx::query_as::<_, UserRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn direct_reports(
    pool: &PgPool,
    org: Uuid,
    manager_id: Uuid,
    locale: &str,
) -> Result<Vec<UserRow>> {
    let sql = format!(
        "{USER_SELECT} WHERE u.organization_id = $1 AND u.manager_id = $3 AND u.deleted_at IS NULL
         ORDER BY u.last_name, u.first_name"
    );
    sqlx::query_as::<_, UserRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(manager_id)
        .fetch_all(pool)
        .await
}

pub struct NewUser<'a> {
    pub email: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub employee_id: Option<&'a str>,
    pub job_title: Option<&'a str>,
    pub department_id: Option<Uuid>,
    pub location_id: Option<Uuid>,
    pub manager_id: Option<Uuid>,
    pub preferred_locale: &'a str,
    pub phone_number: Option<&'a str>,
}

/// Insert an invited row: no password, so the setup link is the only way in.
pub async fn insert_invited(conn: &mut PgConnection, org: Uuid, new: &NewUser<'_>) -> Result<User> {
    sqlx::query_as::<_, User>(
        r#"
        INSERT INTO users (
            organization_id, email, first_name, last_name, employee_id, job_title,
            department_id, location_id, manager_id, preferred_locale, phone_number, status
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'invited')
        RETURNING *
        "#,
    )
    .bind(org)
    .bind(new.email)
    .bind(new.first_name)
    .bind(new.last_name)
    .bind(new.employee_id)
    .bind(new.job_title)
    .bind(new.department_id)
    .bind(new.location_id)
    .bind(new.manager_id)
    .bind(new.preferred_locale)
    .bind(new.phone_number)
    .fetch_one(conn)
    .await
}

pub struct ProfileFields<'a> {
    pub email: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub employee_id: Option<&'a str>,
    pub job_title: Option<&'a str>,
    pub department_id: Option<Uuid>,
    pub location_id: Option<Uuid>,
    pub manager_id: Option<Uuid>,
    pub preferred_locale: &'a str,
    pub phone_number: Option<&'a str>,
    pub avatar_url: Option<&'a str>,
}

pub async fn update_profile(
    conn: &mut PgConnection,
    org: Uuid,
    id: Uuid,
    fields: &ProfileFields<'_>,
) -> Result<Option<User>> {
    sqlx::query_as::<_, User>(
        r#"
        UPDATE users SET
            email = $3, first_name = $4, last_name = $5, employee_id = $6,
            job_title = $7, department_id = $8, location_id = $9, manager_id = $10,
            preferred_locale = $11, phone_number = $12, avatar_url = $13,
            updated_at = now()
        WHERE organization_id = $1 AND id = $2 AND deleted_at IS NULL
        RETURNING *
        "#,
    )
    .bind(org)
    .bind(id)
    .bind(fields.email)
    .bind(fields.first_name)
    .bind(fields.last_name)
    .bind(fields.employee_id)
    .bind(fields.job_title)
    .bind(fields.department_id)
    .bind(fields.location_id)
    .bind(fields.manager_id)
    .bind(fields.preferred_locale)
    .bind(fields.phone_number)
    .bind(fields.avatar_url)
    .fetch_optional(conn)
    .await
}

pub async fn lock_user(conn: &mut PgConnection, org: Uuid, id: Uuid) -> Result<Option<User>> {
    sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE organization_id = $1 AND id = $2 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(org)
    .bind(id)
    .fetch_optional(conn)
    .await
}

pub async fn revoke_sessions(conn: &mut PgConnection, org: Uuid, user_id: Uuid) -> Result<u64> {
    let result = sqlx::query(
        "UPDATE sessions SET revoked_at = now()
         WHERE organization_id = $1 AND user_id = $2 AND revoked_at IS NULL",
    )
    .bind(org)
    .bind(user_id)
    .execute(conn)
    .await?;
    Ok(result.rows_affected())
}

pub async fn set_offboarded(conn: &mut PgConnection, org: Uuid, id: Uuid) -> Result<()> {
    sqlx::query("UPDATE users SET status = 'offboarded', updated_at = now() WHERE organization_id = $1 AND id = $2")
        .bind(org)
        .bind(id)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn soft_delete(conn: &mut PgConnection, org: Uuid, id: Uuid) -> Result<bool> {
    let result = sqlx::query(
        "UPDATE users SET deleted_at = now(), status = 'offboarded', updated_at = now()
         WHERE organization_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(org)
    .bind(id)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn count_direct_reports(conn: &mut PgConnection, org: Uuid, id: Uuid) -> Result<i64> {
    sqlx::query_scalar(
        "SELECT count(*) FROM users WHERE organization_id = $1 AND manager_id = $2 AND deleted_at IS NULL",
    )
    .bind(org)
    .bind(id)
    .fetch_one(conn)
    .await
}

/// True when `manager_id` already reports (directly or transitively) to `id`.
pub async fn would_create_cycle(
    conn: &mut PgConnection,
    org: Uuid,
    id: Uuid,
    manager_id: Uuid,
) -> Result<bool> {
    sqlx::query_scalar(
        r#"
        WITH RECURSIVE chain AS (
            SELECT id, manager_id FROM users
            WHERE organization_id = $1 AND id = $3
            UNION ALL
            SELECT u.id, u.manager_id
            FROM users u JOIN chain c ON u.id = c.manager_id
            WHERE u.organization_id = $1
        )
        SELECT EXISTS (SELECT 1 FROM chain WHERE id = $2)
        "#,
    )
    .bind(org)
    .bind(id)
    .bind(manager_id)
    .fetch_one(conn)
    .await
}

pub async fn reassign_reports(
    conn: &mut PgConnection,
    org: Uuid,
    from: Uuid,
    to: Uuid,
) -> Result<u64> {
    let result = sqlx::query(
        "UPDATE users SET manager_id = $3, updated_at = now()
         WHERE organization_id = $1 AND manager_id = $2 AND deleted_at IS NULL",
    )
    .bind(org)
    .bind(from)
    .bind(to)
    .execute(conn)
    .await?;
    Ok(result.rows_affected())
}

/// Open one pending return per live custody row. Idempotent thanks to the
/// partial unique index.
pub async fn create_return_requests(
    conn: &mut PgConnection,
    org: Uuid,
    user_id: Uuid,
    requested_by: Uuid,
) -> Result<u64> {
    let assets = sqlx::query(
        r#"
        INSERT INTO return_requests (organization_id, user_id, entity_type, entity_id, requested_by)
        SELECT $1, $2, 'asset', a.id, $3
        FROM assets a
        WHERE a.organization_id = $1 AND a.assigned_user_id = $2
        ON CONFLICT (organization_id, entity_type, entity_id) WHERE status = 'pending' DO NOTHING
        "#,
    )
    .bind(org)
    .bind(user_id)
    .bind(requested_by)
    .execute(&mut *conn)
    .await?;

    let accessories = sqlx::query(
        r#"
        INSERT INTO return_requests (organization_id, user_id, entity_type, entity_id, quantity, requested_by)
        SELECT $1, $2, 'accessory', aa.id, aa.quantity - aa.returned_quantity, $3
        FROM accessory_assignments aa
        WHERE aa.organization_id = $1 AND aa.assigned_to_user_id = $2
          AND aa.returned_at IS NULL AND aa.quantity > aa.returned_quantity
        ON CONFLICT (organization_id, entity_type, entity_id) WHERE status = 'pending' DO NOTHING
        "#,
    )
    .bind(org)
    .bind(user_id)
    .bind(requested_by)
    .execute(&mut *conn)
    .await?;

    let licenses = sqlx::query(
        r#"
        INSERT INTO return_requests (organization_id, user_id, entity_type, entity_id, requested_by)
        SELECT $1, $2, 'license', ls.id, $3
        FROM license_seats ls
        WHERE ls.organization_id = $1 AND ls.user_id = $2 AND ls.revoked_at IS NULL
        ON CONFLICT (organization_id, entity_type, entity_id) WHERE status = 'pending' DO NOTHING
        "#,
    )
    .bind(org)
    .bind(user_id)
    .bind(requested_by)
    .execute(&mut *conn)
    .await?;

    Ok(assets.rows_affected() + accessories.rows_affected() + licenses.rows_affected())
}

pub async fn pending_returns(
    pool: &PgPool,
    org: Uuid,
    user_id: Uuid,
) -> Result<Vec<crate::modules::users::dto::PendingReturnResponse>> {
    sqlx::query_as::<_, crate::modules::users::dto::PendingReturnResponse>(
        r#"
        SELECT r.id, r.entity_type, r.entity_id,
               CASE r.entity_type
                   WHEN 'asset' THEN
                       (SELECT a.asset_tag || ' · ' || a.name FROM assets a WHERE a.id = r.entity_id)
                   WHEN 'accessory' THEN
                       (SELECT t.name FROM accessory_assignments aa
                        JOIN accessory_translations t
                          ON t.accessory_id = aa.accessory_id AND t.locale = 'en'
                        WHERE aa.id = r.entity_id)
                   WHEN 'license' THEN
                       (SELECT t.name FROM license_seats ls
                        JOIN license_translations t
                          ON t.license_id = ls.license_id AND t.locale = 'en'
                        WHERE ls.id = r.entity_id)
               END AS label,
               r.quantity, r.status, r.created_at
        FROM return_requests r
        WHERE r.organization_id = $1 AND r.user_id = $2 AND r.status = 'pending'
        ORDER BY r.created_at
        "#,
    )
    .bind(org)
    .bind(user_id)
    .fetch_all(pool)
    .await
}

// ----------------------------------------------------------- import helpers --

pub async fn find_id_by_email(conn: &mut PgConnection, org: Uuid, email: &str) -> Result<Option<Uuid>> {
    sqlx::query_scalar("SELECT id FROM users WHERE organization_id = $1 AND email = $2")
        .bind(org)
        .bind(email)
        .fetch_optional(conn)
        .await
}

/// Resolve a department by code or by any localized name.
pub async fn find_department(conn: &mut PgConnection, org: Uuid, value: &str) -> Result<Option<Uuid>> {
    sqlx::query_scalar(
        r#"
        SELECT d.id FROM departments d
        WHERE d.organization_id = $1
          AND (d.code = upper($2)
               OR EXISTS (SELECT 1 FROM department_translations t
                          WHERE t.department_id = d.id AND t.name ILIKE $2))
        LIMIT 1
        "#,
    )
    .bind(org)
    .bind(value)
    .fetch_optional(conn)
    .await
}

pub async fn insert_department(
    conn: &mut PgConnection,
    org: Uuid,
    code: &str,
    name: &str,
) -> Result<Uuid> {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO departments (organization_id, code) VALUES ($1, $2) RETURNING id",
    )
    .bind(org)
    .bind(code)
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO department_translations (department_id, locale, name) VALUES ($1, 'en', $2)",
    )
    .bind(id)
    .bind(name)
    .execute(&mut *conn)
    .await?;
    Ok(id)
}

pub async fn find_location(conn: &mut PgConnection, org: Uuid, value: &str) -> Result<Option<Uuid>> {
    sqlx::query_scalar(
        r#"
        SELECT l.id FROM locations l
        WHERE l.organization_id = $1
          AND (l.code = upper($2)
               OR EXISTS (SELECT 1 FROM location_translations t
                          WHERE t.location_id = l.id AND t.name ILIKE $2))
        LIMIT 1
        "#,
    )
    .bind(org)
    .bind(value)
    .fetch_optional(conn)
    .await
}
