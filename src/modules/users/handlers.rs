// Employee directory handlers: validate -> RBAC -> repo -> audit -> JSON.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::Json;
use axum::extract::multipart::Multipart;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use chrono::{Duration, Utc};
use serde_json::json;
use uuid::Uuid;

use crate::config::database::{Database, DatabaseTrait};
use crate::error::api_error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::{client_ip, rbac};
use crate::modules::common::{ensure_org_ref, next_available_code, validate_enum};
use crate::modules::users::dto::*;
use crate::modules::users::repo;
use crate::repository::{audit, auth_token, role, user as user_repo};
use crate::response::api_response::ApiResponse;
use crate::service;

const LOCALES: [&str; 3] = ["en", "fr", "ar"];
const STATUSES: [&str; 4] = ["invited", "active", "suspended", "offboarded"];
const SETUP_TOKEN_HOURS: i64 = 72;
const MAX_IMPORT_BYTES: usize = 10 * 1024 * 1024;
const MAX_IMPORT_ROWS: usize = 1000;

fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

fn validate_email(email: &str) -> Result<(), ApiError> {
    if crate::modules::common::valid_email(email) {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "email",
            "invalid",
            "Email is not valid",
        ))
    }
}

fn validate_locale(locale: &str) -> Result<(), ApiError> {
    validate_enum("preferred_locale", locale, &LOCALES)
}

fn validate_required_name(value: &str, field: &str) -> Result<String, ApiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ApiError::bad_request(field, "required", format!("{field} is required")));
    }
    Ok(trimmed.to_string())
}

fn user_response(row: repo::UserRow) -> UserResponse {
    UserResponse {
        full_name: format!("{} {}", row.first_name, row.last_name)
            .trim()
            .to_string(),
        id: row.id,
        email: row.email,
        first_name: row.first_name,
        last_name: row.last_name,
        employee_id: row.employee_id,
        job_title: row.job_title,
        department_id: row.department_id,
        department_name: row.department_name,
        location_id: row.location_id,
        location_name: row.location_name,
        manager_id: row.manager_id,
        manager_name: row.manager_name,
        preferred_locale: row.preferred_locale,
        status: row.status,
        avatar_url: row.avatar_url,
        phone_number: row.phone_number,
        email_verified_at: row.email_verified_at,
        last_login_at: row.last_login_at,
        created_at: row.created_at,
    }
}

fn locale_of(ctx: &AuthContext) -> String {
    ctx.user.preferred_locale.clone()
}

// ------------------------------------------------------------------ reads --

pub async fn list_users(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Query(query): Query<ListUsersQuery>,
) -> Result<ApiResponse<Vec<UserResponse>>, ApiError> {
    rbac::require(&ctx, "users.read")?;
    if let Some(status) = query.status.as_deref() {
        validate_enum("status", status, &STATUSES)?;
    }

    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(25).clamp(1, 100);
    let (rows, total) = repo::list_users(
        db.get_pool(),
        ctx.organization_id(),
        &locale_of(&ctx),
        query
            .search
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty()),
        query.department_id,
        query.location_id,
        query.status.as_deref(),
        query.manager_id,
        query.role_id,
        per_page as i64,
        ((page - 1) * per_page) as i64,
    )
    .await?;

    Ok(ApiResponse::paginated(
        rows.into_iter().map(user_response).collect(),
        page,
        per_page,
        total as u64,
    ))
}

pub async fn get_user(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<UserResponse>, ApiError> {
    rbac::require(&ctx, "users.read")?;
    let row = repo::find_by_id(db.get_pool(), ctx.organization_id(), id, &locale_of(&ctx))
        .await?
        .ok_or(ApiError::NotFound("User"))?;
    Ok(ApiResponse::success(user_response(row)))
}

pub async fn direct_reports(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<Vec<UserResponse>>, ApiError> {
    rbac::require(&ctx, "users.read")?;
    let rows = repo::direct_reports(db.get_pool(), ctx.organization_id(), id, &locale_of(&ctx)).await?;
    Ok(ApiResponse::success(
        rows.into_iter().map(user_response).collect(),
    ))
}

pub async fn pending_returns(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<Vec<PendingReturnResponse>>, ApiError> {
    rbac::require(&ctx, "users.read")?;
    let rows = repo::pending_returns(db.get_pool(), ctx.organization_id(), id).await?;
    Ok(ApiResponse::success(rows))
}

// ---------------------------------------------------------------- writes --

pub async fn create_user(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    headers: HeaderMap,
    Json(payload): Json<CreateUserRequest>,
) -> Result<ApiResponse<CreateUserResponse>, ApiError> {
    rbac::require(&ctx, "users.create")?;
    let org = ctx.organization_id();
    let email = normalize_email(&payload.email);
    validate_email(&email)?;
    let first_name = validate_required_name(&payload.first_name, "first_name")?;
    let last_name = validate_required_name(&payload.last_name, "last_name")?;
    let locale = payload.preferred_locale.as_deref().unwrap_or("en");
    validate_locale(locale)?;

    let pool = db.get_pool();
    let mut tx = pool.begin().await?;

    if repo::find_id_by_email(&mut tx, org, &email).await?.is_some() {
        return Err(ApiError::Conflict("A user with this email already exists".to_string()));
    }
    if let Some(department_id) = payload.department_id {
        ensure_org_ref(&mut tx, "departments", org, department_id, "department_id").await?;
    }
    if let Some(location_id) = payload.location_id {
        ensure_org_ref(&mut tx, "locations", org, location_id, "location_id").await?;
    }
    if let Some(manager_id) = payload.manager_id {
        ensure_org_ref(&mut tx, "users", org, manager_id, "manager_id").await?;
    }

    let created = repo::insert_invited(
        &mut tx,
        org,
        &repo::NewUser {
            email: &email,
            first_name: &first_name,
            last_name: &last_name,
            employee_id: payload.employee_id.as_deref(),
            job_title: payload.job_title.as_deref(),
            department_id: payload.department_id,
            location_id: payload.location_id,
            manager_id: payload.manager_id,
            preferred_locale: locale,
            phone_number: payload.phone_number.as_deref(),
        },
    )
    .await?;

    let role_ids = match payload.role_ids {
        Some(ids) if !ids.is_empty() => ids,
        _ => vec![role::find_by_code(pool, "employee")
            .await?
            .ok_or_else(|| ApiError::Internal("System role 'employee' is missing".to_string()))?
            .id],
    };
    for role_id in role_ids {
        if role::find_for_org(pool, org, role_id).await?.is_none() {
            return Err(ApiError::bad_request(
                "role_ids",
                "not_found",
                "Role does not exist in this organization",
            ));
        }
        role::assign(&mut tx, org, created.id, role_id, Some(ctx.user.id)).await?;
    }

    audit::insert_tx(
        &mut tx,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: Some(ctx.user.id),
            action: "created",
            entity_type: "user",
            entity_id: Some(created.id),
            metadata: json!({ "email": email, "source": "directory" }),
            ip_address: client_ip(&headers).as_deref(),
        },
    )
    .await?;

    tx.commit().await?;

    let token = service::generate_token();
    let expires_at = Utc::now() + Duration::hours(SETUP_TOKEN_HOURS);
    auth_token::replace_live(
        pool,
        org,
        created.id,
        "invite",
        &service::hash_token(&token),
        expires_at,
    )
    .await?;

    let row = repo::find_by_id(pool, org, created.id, locale)
        .await?
        .ok_or(ApiError::NotFound("User"))?;

    Ok(ApiResponse::success(CreateUserResponse {
        user: user_response(row),
        setup_link: format!(
            "{}/setup-account?token={token}",
            crate::handler::invite_base_url().trim_end_matches('/')
        ),
        expires_at,
    }))
}

pub async fn update_user(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateUserRequest>,
) -> Result<ApiResponse<UserResponse>, ApiError> {
    rbac::require(&ctx, "users.update")?;
    let org = ctx.organization_id();
    let pool = db.get_pool();

    let mut tx = pool.begin().await?;
    let existing = user_repo::find_by_id(pool, org, id)
        .await?
        .ok_or(ApiError::NotFound("User"))?;

    let email = payload
        .email
        .as_deref()
        .map(normalize_email)
        .unwrap_or_else(|| existing.email.clone());
    validate_email(&email)?;
    if email != existing.email
        && repo::find_id_by_email(&mut tx, org, &email).await?.is_some()
    {
        return Err(ApiError::Conflict("A user with this email already exists".to_string()));
    }

    let first_name = payload
        .first_name
        .as_deref()
        .map(|v| validate_required_name(v, "first_name"))
        .transpose()?
        .unwrap_or_else(|| existing.first_name.clone());
    let last_name = payload
        .last_name
        .as_deref()
        .map(|v| validate_required_name(v, "last_name"))
        .transpose()?
        .unwrap_or_else(|| existing.last_name.clone());
    let locale = payload
        .preferred_locale
        .as_deref()
        .unwrap_or(&existing.preferred_locale)
        .to_string();
    validate_locale(&locale)?;

    let employee_id = payload
        .employee_id
        .unwrap_or_else(|| existing.employee_id.clone());
    let job_title = payload.job_title.unwrap_or_else(|| existing.job_title.clone());
    let department_id = payload.department_id.unwrap_or(existing.department_id);
    let location_id = payload.location_id.unwrap_or(existing.location_id);
    let manager_id = payload.manager_id.unwrap_or(existing.manager_id);
    let phone_number = payload.phone_number.unwrap_or_else(|| existing.phone_number.clone());
    let avatar_url = payload.avatar_url.unwrap_or_else(|| existing.avatar_url.clone());

    if let Some(department_id) = department_id {
        ensure_org_ref(&mut tx, "departments", org, department_id, "department_id").await?;
    }
    if let Some(location_id) = location_id {
        ensure_org_ref(&mut tx, "locations", org, location_id, "location_id").await?;
    }
    if manager_id == Some(id) {
        return Err(ApiError::bad_request(
            "manager_id",
            "invalid",
            "A user cannot manage themselves",
        ));
    }
    if let Some(manager_id) = manager_id {
        ensure_org_ref(&mut tx, "users", org, manager_id, "manager_id").await?;
        if repo::would_create_cycle(&mut tx, org, id, manager_id).await? {
            return Err(ApiError::bad_request(
                "manager_id",
                "cycle",
                "Manager assignment creates a reporting cycle",
            ));
        }
    }

    repo::update_profile(
        &mut tx,
        org,
        id,
        &repo::ProfileFields {
            email: &email,
            first_name: &first_name,
            last_name: &last_name,
            employee_id: employee_id.as_deref(),
            job_title: job_title.as_deref(),
            department_id,
            location_id,
            manager_id,
            preferred_locale: &locale,
            phone_number: phone_number.as_deref(),
            avatar_url: avatar_url.as_deref(),
        },
    )
    .await?;

    audit::insert_tx(
        &mut tx,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: Some(ctx.user.id),
            action: "updated",
            entity_type: "user",
            entity_id: Some(id),
            metadata: json!({ "source": "directory" }),
            ip_address: None,
        },
    )
    .await?;

    tx.commit().await?;

    let row = repo::find_by_id(pool, org, id, &locale)
        .await?
        .ok_or(ApiError::NotFound("User"))?;
    Ok(ApiResponse::success(user_response(row)))
}

pub async fn delete_user(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<serde_json::Value>, ApiError> {
    rbac::require(&ctx, "users.delete")?;
    if id == ctx.user.id {
        return Err(ApiError::Conflict("Cannot delete your own account".to_string()));
    }
    let org = ctx.organization_id();
    let mut tx = db.get_pool().begin().await?;

    if !repo::soft_delete(&mut tx, org, id).await? {
        return Err(ApiError::NotFound("User"));
    }
    repo::revoke_sessions(&mut tx, org, id).await?;
    audit::insert_tx(
        &mut tx,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: Some(ctx.user.id),
            action: "deleted",
            entity_type: "user",
            entity_id: Some(id),
            metadata: json!({}),
            ip_address: None,
        },
    )
    .await?;
    tx.commit().await?;

    Ok(ApiResponse::success_with_message(json!({}), "User deleted"))
}

pub async fn offboard_user(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
    Json(payload): Json<OffboardRequest>,
) -> Result<ApiResponse<OffboardResponse>, ApiError> {
    rbac::require(&ctx, "users.offboard")?;
    if id == ctx.user.id {
        return Err(ApiError::Conflict("Cannot offboard your own account".to_string()));
    }
    let org = ctx.organization_id();
    let pool = db.get_pool();
    let mut tx = pool.begin().await?;

    let target = repo::lock_user(&mut tx, org, id)
        .await?
        .ok_or(ApiError::NotFound("User"))?;
    if target.status == "offboarded" {
        return Err(ApiError::Conflict("User is already offboarded".to_string()));
    }

    let reports = repo::count_direct_reports(&mut tx, org, id).await?;
    let mut reassigned = 0;
    if reports > 0 {
        let new_manager_id = payload.new_manager_id.ok_or_else(|| {
            ApiError::bad_request(
                "new_manager_id",
                "required",
                "This manager has direct reports; provide new_manager_id",
            )
        })?;
        if new_manager_id == id {
            return Err(ApiError::bad_request(
                "new_manager_id",
                "invalid",
                "The offboarded manager cannot receive their own reports",
            ));
        }
        let new_manager = repo::lock_user(&mut tx, org, new_manager_id)
            .await?
            .ok_or(ApiError::bad_request(
                "new_manager_id",
                "not_found",
                "New manager does not exist in this organization",
            ))?;
        if new_manager.status != "active" {
            return Err(ApiError::bad_request(
                "new_manager_id",
                "inactive",
                "New manager must be active",
            ));
        }
        reassigned = repo::reassign_reports(&mut tx, org, id, new_manager_id).await?;
    }

    repo::set_offboarded(&mut tx, org, id).await?;
    repo::revoke_sessions(&mut tx, org, id).await?;
    let created = repo::create_return_requests(&mut tx, org, id, ctx.user.id).await?;

    audit::insert_tx(
        &mut tx,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: Some(ctx.user.id),
            action: "offboarded",
            entity_type: "user",
            entity_id: Some(id),
            metadata: json!({
                "reassigned_reports": reassigned,
                "return_requests": created,
                "notes": payload.notes,
            }),
            ip_address: None,
        },
    )
    .await?;
    tx.commit().await?;

    let row = repo::find_by_id(pool, org, id, &locale_of(&ctx))
        .await?
        .ok_or(ApiError::NotFound("User"))?;
    let pending = repo::pending_returns(pool, org, id).await?;

    Ok(ApiResponse::success_with_message(
        OffboardResponse {
            user: user_response(row),
            reassigned_reports: reassigned,
            return_requests_created: created,
            pending_returns: pending,
        },
        "User offboarded",
    ))
}

// ---------------------------------------------------------------- import --

/// CSV/XLSX columns (header names are case-insensitive):
/// first_name, last_name, email, employee_id, job_title, department,
/// location, manager_email, preferred_locale, phone_number.
pub async fn import_users(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    mut multipart: Multipart,
) -> Result<ApiResponse<ImportSummary>, ApiError> {
    rbac::require(&ctx, "users.import")?;
    let org = ctx.organization_id();
    let pool = db.get_pool();

    let mut bytes: Option<Vec<u8>> = None;
    let mut file_name = String::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad_request("file", "invalid", e.to_string()))?
    {
        if field.name() != Some("file") {
            continue;
        }
        file_name = field.file_name().unwrap_or_default().to_lowercase();
        let body = field
            .bytes()
            .await
            .map_err(|e| ApiError::bad_request("file", "invalid", e.to_string()))?;
        if body.len() > MAX_IMPORT_BYTES {
            return Err(ApiError::bad_request(
                "file",
                "too_large",
                "File must be 10 MB or smaller",
            ));
        }
        bytes = Some(body.to_vec());
    }

    let bytes = bytes.ok_or_else(|| {
        ApiError::bad_request("file", "required", "Attach a CSV or XLSX file in the 'file' field")
    })?;
    if file_name.ends_with(".xls") && !file_name.ends_with(".xlsx") {
        return Err(ApiError::bad_request(
            "file",
            "unsupported_format",
            "Legacy .xls files are not supported; save the workbook as .xlsx or CSV",
        ));
    }
    let (headers, rows) = if file_name.ends_with(".xlsx") {
        xlsx_rows(&bytes)?
    } else {
        csv_rows(&bytes)?
    };
    for required in ["first_name", "last_name", "email"] {
        if !headers.iter().any(|h| h == required) {
            return Err(ApiError::bad_request(
                "file",
                "invalid",
                format!("Missing required column '{required}'"),
            ));
        }
    }
    if rows.len() > MAX_IMPORT_ROWS {
        return Err(ApiError::bad_request(
            "file",
            "too_many_rows",
            format!("Import is limited to {MAX_IMPORT_ROWS} rows per file"),
        ));
    }

    let mut summary = ImportSummary {
        total_rows: 0,
        imported: 0,
        failed: 0,
        errors: Vec::new(),
    };
    let mut seen_emails: HashSet<String> = HashSet::new();

    for row in rows {
        summary.total_rows += 1;
        let row_number = row.number;
        if let Some(error) = row.error {
            summary.failed += 1;
            summary.errors.push(ImportRowError {
                row: row_number,
                email: None,
                error,
            });
            continue;
        }

        let cells: HashMap<String, String> = headers
            .iter()
            .enumerate()
            .filter_map(|(i, name)| {
                row.cells
                    .get(i)
                    .map(String::as_str)
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(|v| (name.clone(), v.to_string()))
            })
            .collect();

        let email = cells.get("email").map(|e| e.to_lowercase());
        let result = match email.as_deref() {
            Some(email) => {
                if !seen_emails.insert(email.to_string()) {
                    Err("Duplicate email in file".to_string())
                } else {
                    import_row(pool, org, &cells, email).await
                }
            }
            None => Err("email is required".to_string()),
        };

        match result {
            Ok(()) => summary.imported += 1,
            Err(error) => {
                summary.failed += 1;
                summary.errors.push(ImportRowError {
                    row: row_number,
                    email,
                    error,
                });
            }
        }
    }

    audit::insert(
        pool,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: Some(ctx.user.id),
            action: "imported",
            entity_type: "user",
            entity_id: None,
            metadata: json!({
                "total_rows": summary.total_rows,
                "imported": summary.imported,
                "failed": summary.failed,
            }),
            ip_address: None,
        },
    )
    .await?;

    Ok(ApiResponse::success_with_message(summary, "Import processed"))
}

/// A data row parsed from either file format, numbered as it appears in the file.
struct RawRow {
    number: usize,
    cells: Vec<String>,
    error: Option<String>,
}

fn csv_rows(bytes: &[u8]) -> Result<(Vec<String>, Vec<RawRow>), ApiError> {
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(bytes);
    let headers = reader
        .headers()
        .map_err(|e| ApiError::bad_request("file", "invalid", format!("Invalid CSV header: {e}")))?
        .iter()
        .map(|h| h.trim().to_lowercase())
        .collect();
    let rows = reader
        .into_records()
        .enumerate()
        .map(|(index, record)| RawRow {
            number: index + 2,
            cells: record
                .as_ref()
                .map(|r| r.iter().map(|c| c.trim().to_string()).collect())
                .unwrap_or_default(),
            error: record
                .err()
                .map(|e| format!("Malformed CSV row: {e}")),
        })
        .collect();
    Ok((headers, rows))
}

fn xlsx_rows(bytes: &[u8]) -> Result<(Vec<String>, Vec<RawRow>), ApiError> {
    use calamine::{Data, Reader, open_workbook_auto_from_rs};
    use std::io::Cursor;

    let mut workbook = open_workbook_auto_from_rs(Cursor::new(bytes.to_vec())).map_err(|e| {
        ApiError::bad_request("file", "invalid", format!("Invalid spreadsheet: {e}"))
    })?;
    let range = workbook
        .worksheet_range_at(0)
        .ok_or_else(|| {
            ApiError::bad_request("file", "invalid", "The workbook has no worksheet")
        })?
        .map_err(|e| ApiError::bad_request("file", "invalid", format!("Invalid spreadsheet: {e}")))?;

    let cell_text = |cell: &Data| match cell {
        Data::Empty => String::new(),
        Data::String(value) => value.clone(),
        other => other.to_string(),
    };

    let mut rows_iter = range.rows();
    let header_row = rows_iter
        .next()
        .ok_or_else(|| ApiError::bad_request("file", "invalid", "The first worksheet is empty"))?;
    let headers: Vec<String> = header_row
        .iter()
        .map(|cell| cell_text(cell).trim().to_lowercase())
        .collect();
    if headers.iter().all(|h| h.is_empty()) {
        return Err(ApiError::bad_request(
            "file",
            "invalid",
            "The first worksheet has no header row",
        ));
    }

    let first_row = range.start().map(|(row, _)| row as usize).unwrap_or(0) + 2;
    let rows = rows_iter
        .enumerate()
        .filter(|(_, cells)| {
            cells
                .iter()
                .any(|cell| !cell_text(cell).trim().is_empty())
        })
        .map(|(index, cells)| RawRow {
            number: first_row + index,
            cells: cells.iter().map(cell_text).collect(),
            error: None,
        })
        .collect();
    Ok((headers, rows))
}

/// One row per transaction: a bad row never rolls back the good ones.
async fn import_row(
    pool: &sqlx::PgPool,
    org: Uuid,
    cells: &HashMap<String, String>,
    email: &str,
) -> Result<(), String> {
    if !crate::modules::common::valid_email(email) {
        return Err("email is not valid".to_string());
    }
    let first_name = cells.get("first_name").ok_or("first_name is required")?;
    let last_name = cells.get("last_name").ok_or("last_name is required")?;
    let locale = cells
        .get("preferred_locale")
        .cloned()
        .unwrap_or_else(|| "en".to_string());
    if !LOCALES.contains(&locale.as_str()) {
        return Err(format!(
            "preferred_locale must be one of: {}",
            LOCALES.join(", ")
        ));
    }

    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

    if repo::find_id_by_email(&mut tx, org, email)
        .await
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Err("email already exists".to_string());
    }

    let department_id = match cells.get("department") {
        Some(value) => Some(resolve_or_create_department(&mut tx, org, value).await?),
        None => None,
    };
    let location_id = match cells.get("location") {
        Some(value) => Some(
            repo::find_location(&mut tx, org, value)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("location '{value}' not found"))?,
        ),
        None => None,
    };
    let manager_id = match cells.get("manager_email") {
        Some(value) => Some(
            repo::find_id_by_email(&mut tx, org, &value.to_lowercase())
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("manager '{value}' not found"))?,
        ),
        None => None,
    };

    let created = repo::insert_invited(
        &mut tx,
        org,
        &repo::NewUser {
            email,
            first_name: first_name.as_str(),
            last_name: last_name.as_str(),
            employee_id: cells.get("employee_id").map(String::as_str),
            job_title: cells.get("job_title").map(String::as_str),
            department_id,
            location_id,
            manager_id,
            preferred_locale: &locale,
            phone_number: cells.get("phone_number").map(String::as_str),
        },
    )
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            "employee_id already exists".to_string()
        }
        _ => e.to_string(),
    })?;

    if let Some(role_id) = role::find_by_code(pool, "employee")
        .await
        .map_err(|e| e.to_string())?
        .map(|r| r.id)
    {
        role::assign(&mut tx, org, created.id, role_id, None)
            .await
            .map_err(|e| e.to_string())?;
    }

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

async fn resolve_or_create_department(
    conn: &mut sqlx::PgConnection,
    org: Uuid,
    value: &str,
) -> Result<Uuid, String> {
    if let Some(id) = repo::find_department(conn, org, value)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(id);
    }
    let base = crate::modules::common::derive_code(value);
    let code = next_available_code(conn, "departments", org, &base)
        .await
        .map_err(|e| e.to_string())?;
    repo::insert_department(conn, org, &code, value)
        .await
        .map_err(|e| e.to_string())
}
