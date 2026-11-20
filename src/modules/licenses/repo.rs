// License data access. Seat allocation locks the license row ("SELECT ... FOR
// UPDATE") before counting active seats, so `active_seats` can never exceed
// `total_seats` even under concurrent requests.

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::api_error::ApiError;
use crate::modules::common::ensure_org_ref;
use crate::modules::i18n::Translations;
use crate::modules::licenses::dto::*;
use crate::repository::audit;

type Result<T> = std::result::Result<T, ApiError>;

fn trimmed(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

async fn replace_translations(
    conn: &mut sqlx::PgConnection,
    license_id: Uuid,
    translations: &Translations,
) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM license_translations WHERE license_id = $1")
        .bind(license_id)
        .execute(&mut *conn)
        .await?;
    for (locale, name) in translations {
        sqlx::query(
            "INSERT INTO license_translations (license_id, locale, name) VALUES ($1, $2, $3)",
        )
        .bind(license_id)
        .bind(locale)
        .bind(name.trim())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

#[derive(Debug, FromRow)]
pub struct LicenseRow {
    pub id: Uuid,
    pub manufacturer_id: Option<Uuid>,
    pub manufacturer_name: Option<String>,
    pub supplier_id: Option<Uuid>,
    pub supplier_name: Option<String>,
    pub license_type: String,
    pub total_seats: i32,
    pub active_seats: i32,
    pub available_seats: i32,
    pub purchase_date: Option<NaiveDate>,
    pub expiration_date: Option<NaiveDate>,
    pub days_until_expiry: Option<i32>,
    pub expiry_bucket: Option<String>,
    pub purchase_cost: Option<Decimal>,
    pub currency: String,
    pub order_number: Option<String>,
    pub product_key: Option<String>,
    pub notes: Option<String>,
    pub is_archived: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[sqlx(json)]
    pub translations: std::collections::BTreeMap<String, String>,
}

/// `$1` organization, `$2` locale. Expiry windows are inclusive of today.
const LICENSE_SELECT: &str = r#"
    SELECT l.id,
           l.manufacturer_id,
           COALESCE((SELECT t.name FROM manufacturer_translations t
                     WHERE t.manufacturer_id = l.manufacturer_id AND t.locale = $2),
                    (SELECT t.name FROM manufacturer_translations t
                     WHERE t.manufacturer_id = l.manufacturer_id AND t.locale = 'en')) AS manufacturer_name,
           l.supplier_id,
           COALESCE((SELECT t.name FROM supplier_translations t
                     WHERE t.supplier_id = l.supplier_id AND t.locale = $2),
                    (SELECT t.name FROM supplier_translations t
                     WHERE t.supplier_id = l.supplier_id AND t.locale = 'en')) AS supplier_name,
           l.license_type, l.total_seats,
           COALESCE(seat.active_seats, 0)::int AS active_seats,
           (l.total_seats - COALESCE(seat.active_seats, 0))::int AS available_seats,
           l.purchase_date, l.expiration_date,
           (l.expiration_date - CURRENT_DATE)::int AS days_until_expiry,
           CASE
               WHEN l.expiration_date IS NULL THEN NULL
               WHEN l.expiration_date < CURRENT_DATE THEN 'expired'
               WHEN l.expiration_date <= CURRENT_DATE + 30 THEN '30'
               WHEN l.expiration_date <= CURRENT_DATE + 60 THEN '60'
               WHEN l.expiration_date <= CURRENT_DATE + 90 THEN '90'
               ELSE NULL
           END AS expiry_bucket,
           l.purchase_cost, l.currency, l.order_number, l.product_key, l.notes, l.is_archived,
           l.created_at, l.updated_at,
           COALESCE((SELECT jsonb_object_agg(t.locale, t.name)
                     FROM license_translations t WHERE t.license_id = l.id), '{}'::jsonb) AS translations
    FROM licenses l
    LEFT JOIN LATERAL (
        SELECT count(*) AS active_seats
        FROM license_seats s
        WHERE s.license_id = l.id AND s.revoked_at IS NULL
    ) seat ON TRUE
    WHERE l.organization_id = $1
"#;

/// `$3` q, `$4` expiring_in_days, `$5` archived.
const LICENSE_FILTER: &str = r#"
      AND ($3::text IS NULL OR l.product_key ILIKE '%' || $3 || '%'
           OR l.order_number ILIKE '%' || $3 || '%'
           OR EXISTS (SELECT 1 FROM license_translations s
                      WHERE s.license_id = l.id AND s.name ILIKE '%' || $3 || '%'))
      AND ($4::int IS NULL OR (l.expiration_date IS NOT NULL
                               AND l.expiration_date >= CURRENT_DATE
                               AND l.expiration_date <= CURRENT_DATE + $4))
      AND ($5::boolean IS NULL OR l.is_archived = $5)
"#;

pub async fn list_licenses(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    query: &LicenseListQuery,
    page: u32,
    per_page: u32,
) -> Result<(Vec<LicenseRow>, i64)> {
    let sql = format!(
        "{LICENSE_SELECT}{LICENSE_FILTER} ORDER BY l.created_at DESC, l.id LIMIT $6 OFFSET $7"
    );
    let rows = sqlx::query_as::<_, LicenseRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(query.q.as_deref())
        .bind(query.expiring_in_days)
        .bind(query.is_archived)
        .bind(per_page as i64)
        .bind(((page.saturating_sub(1)) * per_page) as i64)
        .fetch_all(pool)
        .await?;

    let count_sql =
        format!("SELECT count(*) FROM licenses l WHERE l.organization_id = $1{LICENSE_FILTER}");
    let total: i64 = sqlx::query_scalar(&count_sql)
        .bind(org)
        .bind(locale)
        .bind(query.q.as_deref())
        .bind(query.expiring_in_days)
        .bind(query.is_archived)
        .fetch_one(pool)
        .await?;
    Ok((rows, total))
}

pub async fn get_license(pool: &PgPool, org: Uuid, locale: &str, id: Uuid) -> Result<LicenseRow> {
    let sql = format!("{LICENSE_SELECT} AND l.id = $3");
    sqlx::query_as::<_, LicenseRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("License"))
}

/// Non-archived licenses inside the 30/60/90-day windows, plus expired ones.
pub async fn expiry_alerts(pool: &PgPool, org: Uuid, locale: &str) -> Result<Vec<LicenseRow>> {
    let sql = format!(
        "{LICENSE_SELECT} AND l.is_archived = FALSE AND l.expiration_date IS NOT NULL
         AND l.expiration_date <= CURRENT_DATE + 90
         ORDER BY l.expiration_date, l.id"
    );
    Ok(sqlx::query_as::<_, LicenseRow>(&sql)
        .bind(org)
        .bind(locale)
        .fetch_all(pool)
        .await?)
}

pub async fn create_license(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: &Option<String>,
    req: &CreateLicenseRequest,
) -> Result<LicenseRow> {
    let mut tx = pool.begin().await?;
    audit::bind_actor(
        &mut tx,
        audit::Actor {
            user_id: Some(actor),
            ip_address: ip.as_deref(),
            user_agent: None,
        },
    )
    .await?;
    if let Some(id) = req.manufacturer_id {
        ensure_org_ref(&mut tx, "manufacturers", org, id, "manufacturer_id").await?;
    }
    if let Some(id) = req.supplier_id {
        ensure_org_ref(&mut tx, "suppliers", org, id, "supplier_id").await?;
    }

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO licenses
            (organization_id, manufacturer_id, supplier_id, license_type, total_seats,
             purchase_date, expiration_date, purchase_cost, currency, order_number,
             product_key, notes, is_archived)
         VALUES ($1, $2, $3, COALESCE($4, 'named'), $5, $6, $7, $8, COALESCE($9, 'USD'),
                 $10, $11, $12, $13)
         RETURNING id",
    )
    .bind(org)
    .bind(req.manufacturer_id)
    .bind(req.supplier_id)
    .bind(req.license_type.as_deref())
    .bind(req.total_seats)
    .bind(req.purchase_date)
    .bind(req.expiration_date)
    .bind(req.purchase_cost)
    .bind(trimmed(&req.currency).map(|c| c.to_uppercase()))
    .bind(trimmed(&req.order_number))
    .bind(trimmed(&req.product_key))
    .bind(trimmed(&req.notes))
    .bind(req.is_archived)
    .fetch_one(&mut *tx)
    .await?;

    replace_translations(&mut tx, id, &req.translations).await?;


    tx.commit().await?;
    get_license(pool, org, locale, id).await
}

pub async fn update_license(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: &Option<String>,
    id: Uuid,
    req: &UpdateLicenseRequest,
) -> Result<LicenseRow> {
    let current = get_license(pool, org, locale, id).await?;
    let mut tx = pool.begin().await?;
    audit::bind_actor(
        &mut tx,
        audit::Actor {
            user_id: Some(actor),
            ip_address: ip.as_deref(),
            user_agent: None,
        },
    )
    .await?;

    if let Some(Some(manufacturer_id)) = req.manufacturer_id {
        ensure_org_ref(
            &mut tx,
            "manufacturers",
            org,
            manufacturer_id,
            "manufacturer_id",
        )
        .await?;
    }
    if let Some(Some(supplier_id)) = req.supplier_id {
        ensure_org_ref(&mut tx, "suppliers", org, supplier_id, "supplier_id").await?;
    }

    let total_seats = req.total_seats.unwrap_or(current.total_seats);
    if total_seats < current.active_seats {
        return Err(ApiError::bad_request(
            "total_seats",
            "invalid",
            format!(
                "total_seats cannot be lower than the {} active seat(s)",
                current.active_seats
            ),
        ));
    }

    if let Some(translations) = &req.translations {
        replace_translations(&mut tx, id, translations).await?;
    }

    sqlx::query(
        "UPDATE licenses
         SET manufacturer_id = $3, supplier_id = $4, license_type = $5, total_seats = $6,
             purchase_date = $7, expiration_date = $8, purchase_cost = $9, currency = $10,
             order_number = $11, product_key = $12, notes = $13, is_archived = $14,
             updated_at = now()
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(id)
    .bind(org)
    .bind(req.manufacturer_id.unwrap_or(current.manufacturer_id))
    .bind(req.supplier_id.unwrap_or(current.supplier_id))
    .bind(req.license_type.clone().unwrap_or(current.license_type))
    .bind(total_seats)
    .bind(req.purchase_date.unwrap_or(current.purchase_date))
    .bind(req.expiration_date.unwrap_or(current.expiration_date))
    .bind(req.purchase_cost.unwrap_or(current.purchase_cost))
    .bind(
        req.currency
            .as_deref()
            .map(|c| c.trim().to_uppercase())
            .unwrap_or(current.currency),
    )
    .bind(req.order_number.clone().unwrap_or(current.order_number))
    .bind(req.product_key.clone().unwrap_or(current.product_key))
    .bind(req.notes.clone().unwrap_or(current.notes))
    .bind(req.is_archived.unwrap_or(current.is_archived))
    .execute(&mut *tx)
    .await?;


    tx.commit().await?;
    get_license(pool, org, locale, id).await
}

pub async fn delete_license(
    pool: &PgPool,
    org: Uuid,
    actor: Uuid,
    ip: &Option<String>,
    id: Uuid,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    audit::bind_actor(
        &mut tx,
        audit::Actor {
            user_id: Some(actor),
            ip_address: ip.as_deref(),
            user_agent: None,
        },
    )
    .await?;
    let done = sqlx::query("DELETE FROM licenses WHERE id = $1 AND organization_id = $2")
        .bind(id)
        .bind(org)
        .execute(&mut *tx)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound("License"));
    }
    tx.commit().await?;
    Ok(())
}

// ----------------------------------------------------------------- seats -----

#[derive(Debug, FromRow)]
pub struct SeatRow {
    pub id: Uuid,
    pub license_id: Uuid,
    pub license_name: String,
    pub user_id: Option<Uuid>,
    pub user_name: Option<String>,
    pub asset_id: Option<Uuid>,
    pub asset_tag: Option<String>,
    pub notes: Option<String>,
    pub assigned_by_user_id: Option<Uuid>,
    pub assigned_by_user_name: Option<String>,
    pub revoked_by_user_id: Option<Uuid>,
    pub revoked_by_user_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// `$1` organization, `$2` locale.
const SEAT_SELECT: &str = r#"
    SELECT s.id, s.license_id,
           COALESCE((SELECT t.name FROM license_translations t
                     WHERE t.license_id = s.license_id AND t.locale = $2),
                    (SELECT t.name FROM license_translations t
                     WHERE t.license_id = s.license_id AND t.locale = 'en')) AS license_name,
           s.user_id,
           (SELECT NULLIF(btrim(u.first_name || ' ' || u.last_name), '')
              FROM users u WHERE u.id = s.user_id) AS user_name,
           s.asset_id,
           (SELECT a.asset_tag FROM assets a WHERE a.id = s.asset_id) AS asset_tag,
           s.notes, s.assigned_by_user_id,
           (SELECT NULLIF(btrim(u.first_name || ' ' || u.last_name), '')
              FROM users u WHERE u.id = s.assigned_by_user_id) AS assigned_by_user_name,
           s.revoked_by_user_id,
           (SELECT NULLIF(btrim(u.first_name || ' ' || u.last_name), '')
              FROM users u WHERE u.id = s.revoked_by_user_id) AS revoked_by_user_name,
           s.created_at, s.revoked_at
    FROM license_seats s
    WHERE s.organization_id = $1
"#;

pub async fn list_seats(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    license_id: Uuid,
) -> Result<Vec<SeatRow>> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM licenses WHERE id = $1 AND organization_id = $2)",
    )
    .bind(license_id)
    .bind(org)
    .fetch_one(pool)
    .await?;
    if !exists {
        return Err(ApiError::NotFound("License"));
    }
    let sql = format!("{SEAT_SELECT} AND s.license_id = $3 ORDER BY s.created_at DESC, s.id");
    Ok(sqlx::query_as::<_, SeatRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(license_id)
        .fetch_all(pool)
        .await?)
}

pub async fn get_seat(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    license_id: Uuid,
    seat_id: Uuid,
) -> Result<SeatRow> {
    let sql = format!("{SEAT_SELECT} AND s.license_id = $3 AND s.id = $4");
    sqlx::query_as::<_, SeatRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(license_id)
        .bind(seat_id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("License seat"))
}

/// Active seats held by a user (360 view).
pub async fn list_active_for_user(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    user_id: Uuid,
) -> Result<Vec<SeatRow>> {
    let sql = format!(
        "{SEAT_SELECT} AND s.user_id = $3 AND s.revoked_at IS NULL ORDER BY s.created_at DESC, s.id"
    );
    Ok(sqlx::query_as::<_, SeatRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(user_id)
        .fetch_all(pool)
        .await?)
}

/// Active seats held by a hardware asset (360 view).
pub async fn list_active_for_asset(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    asset_id: Uuid,
) -> Result<Vec<SeatRow>> {
    let sql = format!(
        "{SEAT_SELECT} AND s.asset_id = $3 AND s.revoked_at IS NULL ORDER BY s.created_at DESC, s.id"
    );
    Ok(sqlx::query_as::<_, SeatRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(asset_id)
        .fetch_all(pool)
        .await?)
}

pub async fn allocate_seat(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: &Option<String>,
    license_id: Uuid,
    req: &AllocateSeatRequest,
) -> Result<SeatRow> {
    if req.user_id.is_some() == req.asset_id.is_some() {
        return Err(ApiError::bad_request(
            "user_id",
            "invalid",
            "Provide exactly one of user_id or asset_id",
        ));
    }

    let mut tx = pool.begin().await?;
    audit::bind_actor(
        &mut tx,
        audit::Actor {
            user_id: Some(actor),
            ip_address: ip.as_deref(),
            user_agent: None,
        },
    )
    .await?;

    // Lock the license first; every allocator serialises behind it.
    let total_seats: Option<i32> = sqlx::query_scalar(
        "SELECT total_seats FROM licenses WHERE id = $1 AND organization_id = $2 FOR UPDATE",
    )
    .bind(license_id)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    let total_seats = total_seats.ok_or(ApiError::NotFound("License"))?;

    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM license_seats
         WHERE license_id = $1 AND organization_id = $2 AND revoked_at IS NULL",
    )
    .bind(license_id)
    .bind(org)
    .fetch_one(&mut *tx)
    .await?;
    if active >= i64::from(total_seats) {
        return Err(ApiError::Conflict(format!(
            "All {total_seats} seat(s) are already allocated"
        )));
    }

    if let Some(user_id) = req.user_id {
        ensure_org_ref(&mut tx, "users", org, user_id, "user_id").await?;
    }
    if let Some(asset_id) = req.asset_id {
        ensure_org_ref(&mut tx, "assets", org, asset_id, "asset_id").await?;
    }

    let seat_id: Uuid = sqlx::query_scalar(
        "INSERT INTO license_seats
            (organization_id, license_id, user_id, asset_id, notes, assigned_by_user_id)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(org)
    .bind(license_id)
    .bind(req.user_id)
    .bind(req.asset_id)
    .bind(trimmed(&req.notes))
    .bind(actor)
    .fetch_one(&mut *tx)
    .await?;

    audit::insert_tx(
        &mut tx,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: Some(actor),
            action: "assigned",
            entity_type: "license",
            entity_id: Some(license_id),
            metadata: serde_json::json!({
                "seat_id": seat_id,
                "user_id": req.user_id,
                "asset_id": req.asset_id,
            }),
            ip_address: ip.as_deref(),
        },
    )
    .await?;

    tx.commit().await?;
    get_seat(pool, org, locale, license_id, seat_id).await
}

pub async fn revoke_seat(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    actor: Uuid,
    ip: &Option<String>,
    license_id: Uuid,
    seat_id: Uuid,
    req: &RevokeSeatRequest,
) -> Result<SeatRow> {
    let mut tx = pool.begin().await?;
    audit::bind_actor(
        &mut tx,
        audit::Actor {
            user_id: Some(actor),
            ip_address: ip.as_deref(),
            user_agent: None,
        },
    )
    .await?;

    let done = sqlx::query(
        "UPDATE license_seats
         SET revoked_at = now(), revoked_by_user_id = $4, notes = COALESCE($5, notes)
         WHERE id = $1 AND license_id = $2 AND organization_id = $3 AND revoked_at IS NULL",
    )
    .bind(seat_id)
    .bind(license_id)
    .bind(org)
    .bind(actor)
    .bind(trimmed(&req.notes))
    .execute(&mut *tx)
    .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound("Active license seat"));
    }

    audit::insert_tx(
        &mut tx,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: Some(actor),
            action: "revoked",
            entity_type: "license",
            entity_id: Some(license_id),
            metadata: serde_json::json!({ "seat_id": seat_id }),
            ip_address: ip.as_deref(),
        },
    )
    .await?;

    tx.commit().await?;
    get_seat(pool, org, locale, license_id, seat_id).await
}
