// Lookup handlers: validate -> RBAC -> repo -> JSON.
//
// Lookups are small per-tenant lists, so they are returned whole; `?q=` does a
// case-insensitive search across the localized names.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use uuid::Uuid;

use crate::config::database::{Database, DatabaseTrait};
use crate::error::api_error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::rbac;
use crate::modules::common::validate_enum;
use crate::modules::i18n::{self, Locale, Translations};
use crate::modules::lookups::dto::*;
use crate::modules::lookups::repo;
use crate::response::api_response::ApiResponse;

pub const CATEGORY_TYPES: [&str; 4] = ["accessory", "asset", "consumable", "component"];
pub const STATUS_TYPES: [&str; 5] = [
    "deployable",
    "deployed",
    "undeployable",
    "pending",
    "archived",
];
pub const LOCATION_TYPES: [&str; 5] = ["site", "building", "floor", "room", "shelf"];

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub q: Option<String>,
}

fn validate_translations(translations: &Translations) -> Result<(), ApiError> {
    i18n::validate(translations)
        .map_err(|message| ApiError::bad_request("translations", "invalid", message))
}

fn validate_code(
    code: &str,
    max_len: usize,
    allow_dash: bool,
    field: &str,
) -> Result<(), ApiError> {
    let ok = (1..=max_len).contains(&code.len())
        && code
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || (allow_dash && b == b'-'));
    if ok {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            field,
            "invalid",
            "Code uses uppercase letters, digits or dashes and is too long",
        ))
    }
}

/// `^#[0-9A-Fa-f]{6}$`
fn validate_color(color: &str) -> Result<(), ApiError> {
    let ok = color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|b| b.is_ascii_hexdigit());
    if ok {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "color_code",
            "invalid",
            "color_code must look like #RRGGBB",
        ))
    }
}

// ------------------------------------------------------------ categories ----

fn category_response(row: repo::CategoryRow, locale: &str) -> CategoryResponse {
    CategoryResponse {
        name: i18n::pick(&row.translations, locale),
        id: row.id,
        code: row.code,
        translations: row.translations,
        category_type: row.category_type,
        skip_checkout_confirmation_emails: row.skip_checkout_confirmation_emails,
        icon_url: row.icon_url,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

pub async fn list_categories(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Query(query): Query<ListQuery>,
) -> Result<ApiResponse<Vec<CategoryResponse>>, ApiError> {
    rbac::require(&ctx, "lookups.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let rows =
        repo::list_categories(db.get_pool(), ctx.organization_id(), query.q.as_deref()).await?;
    let items = rows
        .into_iter()
        .map(|row| category_response(row, &locale))
        .collect();
    Ok(ApiResponse::success(items))
}

pub async fn get_category(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<CategoryResponse>, ApiError> {
    rbac::require(&ctx, "lookups.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let row = repo::get_category(db.get_pool(), ctx.organization_id(), id).await?;
    Ok(ApiResponse::success(category_response(row, &locale)))
}

pub async fn create_category(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Json(payload): Json<CreateCategoryRequest>,
) -> Result<ApiResponse<CategoryResponse>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    validate_translations(&payload.translations)?;
    validate_enum("category_type", &payload.category_type, &CATEGORY_TYPES)?;
    if let Some(code) = payload.code.as_deref() {
        validate_code(&code.to_uppercase(), 8, false, "code")?;
    }
    let row = repo::create_category(db.get_pool(), ctx.organization_id(), &payload).await?;
    Ok(ApiResponse::success_with_message(
        category_response(row, &locale),
        "Category created",
    ))
}

pub async fn update_category(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateCategoryRequest>,
) -> Result<ApiResponse<CategoryResponse>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    if let Some(translations) = &payload.translations {
        validate_translations(translations)?;
    }
    if let Some(category_type) = payload.category_type.as_deref() {
        validate_enum("category_type", category_type, &CATEGORY_TYPES)?;
    }
    if let Some(code) = payload.code.as_deref() {
        validate_code(&code.to_uppercase(), 8, false, "code")?;
    }
    let row = repo::update_category(db.get_pool(), ctx.organization_id(), id, &payload).await?;
    Ok(ApiResponse::success(category_response(row, &locale)))
}

pub async fn delete_category(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<String>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    repo::delete_category(db.get_pool(), ctx.organization_id(), id).await?;
    Ok(ApiResponse::success("Category deleted".to_string()))
}

// --------------------------------------------------------- manufacturers ----

fn manufacturer_response(row: repo::ManufacturerRow, locale: &str) -> ManufacturerResponse {
    ManufacturerResponse {
        name: i18n::pick(&row.translations, locale),
        id: row.id,
        translations: row.translations,
        url: row.url,
        support_url: row.support_url,
        support_phone: row.support_phone,
        support_email: row.support_email,
        notes: row.notes,
        logo_url: row.logo_url,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

pub async fn list_manufacturers(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Query(query): Query<ListQuery>,
) -> Result<ApiResponse<Vec<ManufacturerResponse>>, ApiError> {
    rbac::require(&ctx, "lookups.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let rows =
        repo::list_manufacturers(db.get_pool(), ctx.organization_id(), query.q.as_deref()).await?;
    Ok(ApiResponse::success(
        rows.into_iter()
            .map(|row| manufacturer_response(row, &locale))
            .collect(),
    ))
}

pub async fn get_manufacturer(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<ManufacturerResponse>, ApiError> {
    rbac::require(&ctx, "lookups.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let row = repo::get_manufacturer(db.get_pool(), ctx.organization_id(), id).await?;
    Ok(ApiResponse::success(manufacturer_response(row, &locale)))
}

pub async fn create_manufacturer(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Json(payload): Json<CreateManufacturerRequest>,
) -> Result<ApiResponse<ManufacturerResponse>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    validate_translations(&payload.translations)?;
    let row = repo::create_manufacturer(db.get_pool(), ctx.organization_id(), &payload).await?;
    Ok(ApiResponse::success_with_message(
        manufacturer_response(row, &locale),
        "Manufacturer created",
    ))
}

pub async fn update_manufacturer(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateManufacturerRequest>,
) -> Result<ApiResponse<ManufacturerResponse>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    if let Some(translations) = &payload.translations {
        validate_translations(translations)?;
    }
    let row = repo::update_manufacturer(db.get_pool(), ctx.organization_id(), id, &payload).await?;
    Ok(ApiResponse::success(manufacturer_response(row, &locale)))
}

pub async fn delete_manufacturer(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<String>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    repo::delete_manufacturer(db.get_pool(), ctx.organization_id(), id).await?;
    Ok(ApiResponse::success("Manufacturer deleted".to_string()))
}

// --------------------------------------------------------------- statuses ----

fn status_response(row: repo::StatusRow, locale: &str) -> StatusResponse {
    StatusResponse {
        name: i18n::pick(&row.translations, locale),
        id: row.id,
        code: row.code,
        translations: row.translations,
        status_type: row.status_type,
        color_code: row.color_code,
        show_in_nav: row.show_in_nav,
        notes: row.notes,
        assets_count: row.assets_count,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

pub async fn list_statuses(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Query(query): Query<ListQuery>,
) -> Result<ApiResponse<Vec<StatusResponse>>, ApiError> {
    rbac::require(&ctx, "lookups.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let rows =
        repo::list_statuses(db.get_pool(), ctx.organization_id(), query.q.as_deref()).await?;
    Ok(ApiResponse::success(
        rows.into_iter()
            .map(|row| status_response(row, &locale))
            .collect(),
    ))
}

pub async fn get_status(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<StatusResponse>, ApiError> {
    rbac::require(&ctx, "lookups.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let row = repo::get_status(db.get_pool(), ctx.organization_id(), id).await?;
    Ok(ApiResponse::success(status_response(row, &locale)))
}

pub async fn create_status(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Json(payload): Json<CreateStatusRequest>,
) -> Result<ApiResponse<StatusResponse>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    validate_translations(&payload.translations)?;
    validate_enum("status_type", &payload.status_type, &STATUS_TYPES)?;
    if let Some(color) = payload.color_code.as_deref() {
        validate_color(color)?;
    }
    let row = repo::create_status(db.get_pool(), ctx.organization_id(), &payload).await?;
    Ok(ApiResponse::success_with_message(
        status_response(row, &locale),
        "Status created",
    ))
}

pub async fn update_status(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateStatusRequest>,
) -> Result<ApiResponse<StatusResponse>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    if let Some(translations) = &payload.translations {
        validate_translations(translations)?;
    }
    if let Some(status_type) = payload.status_type.as_deref() {
        validate_enum("status_type", status_type, &STATUS_TYPES)?;
    }
    if let Some(color) = payload.color_code.as_deref() {
        validate_color(color)?;
    }
    let row = repo::update_status(db.get_pool(), ctx.organization_id(), id, &payload).await?;
    Ok(ApiResponse::success(status_response(row, &locale)))
}

pub async fn delete_status(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<String>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    repo::delete_status(db.get_pool(), ctx.organization_id(), id).await?;
    Ok(ApiResponse::success("Status deleted".to_string()))
}

// ------------------------------------------------------------ departments ----

fn department_response(row: repo::DepartmentRow, locale: &str) -> DepartmentResponse {
    DepartmentResponse {
        name: i18n::pick(&row.translations, locale),
        id: row.id,
        code: row.code,
        translations: row.translations,
        email: row.email,
        phone_number: row.phone_number,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

pub async fn list_departments(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Query(query): Query<ListQuery>,
) -> Result<ApiResponse<Vec<DepartmentResponse>>, ApiError> {
    rbac::require(&ctx, "lookups.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let rows =
        repo::list_departments(db.get_pool(), ctx.organization_id(), query.q.as_deref()).await?;
    Ok(ApiResponse::success(
        rows.into_iter()
            .map(|row| department_response(row, &locale))
            .collect(),
    ))
}

pub async fn get_department(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<DepartmentResponse>, ApiError> {
    rbac::require(&ctx, "lookups.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let row = repo::get_department(db.get_pool(), ctx.organization_id(), id).await?;
    Ok(ApiResponse::success(department_response(row, &locale)))
}

pub async fn create_department(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Json(payload): Json<CreateDepartmentRequest>,
) -> Result<ApiResponse<DepartmentResponse>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    validate_translations(&payload.translations)?;
    if let Some(code) = payload.code.as_deref() {
        validate_code(&code.to_uppercase(), 8, false, "code")?;
    }
    let row = repo::create_department(db.get_pool(), ctx.organization_id(), &payload).await?;
    Ok(ApiResponse::success_with_message(
        department_response(row, &locale),
        "Department created",
    ))
}

pub async fn update_department(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateDepartmentRequest>,
) -> Result<ApiResponse<DepartmentResponse>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    if let Some(translations) = &payload.translations {
        validate_translations(translations)?;
    }
    if let Some(code) = payload.code.as_deref() {
        validate_code(&code.to_uppercase(), 8, false, "code")?;
    }
    let row = repo::update_department(db.get_pool(), ctx.organization_id(), id, &payload).await?;
    Ok(ApiResponse::success(department_response(row, &locale)))
}

pub async fn delete_department(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<String>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    repo::delete_department(db.get_pool(), ctx.organization_id(), id).await?;
    Ok(ApiResponse::success("Department deleted".to_string()))
}

// -------------------------------------------------------------- locations ----

fn location_response(row: repo::LocationRow, locale: &str) -> LocationResponse {
    LocationResponse {
        name: i18n::pick(&row.translations, locale),
        id: row.id,
        code: row.code,
        translations: row.translations,
        parent_id: row.parent_id,
        location_type: row.location_type,
        address_line1: row.address_line1,
        address_line2: row.address_line2,
        city: row.city,
        state_region: row.state_region,
        postal_code: row.postal_code,
        country_code: row.country_code,
        latitude: row.latitude,
        longitude: row.longitude,
        phone_number: row.phone_number,
        email: row.email,
        manager_user_id: row.manager_user_id,
        notes: row.notes,
        is_active: row.is_active,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn validate_country_code(code: Option<&str>) -> Result<(), ApiError> {
    match code {
        Some(code) if code.len() != 2 || !code.bytes().all(|b| b.is_ascii_alphabetic()) => {
            Err(ApiError::bad_request(
                "country_code",
                "invalid",
                "country_code must be a 2-letter ISO-3166 code",
            ))
        }
        _ => Ok(()),
    }
}

pub async fn list_locations(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Query(query): Query<ListQuery>,
) -> Result<ApiResponse<Vec<LocationResponse>>, ApiError> {
    rbac::require(&ctx, "lookups.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let rows =
        repo::list_locations(db.get_pool(), ctx.organization_id(), query.q.as_deref()).await?;
    Ok(ApiResponse::success(
        rows.into_iter()
            .map(|row| location_response(row, &locale))
            .collect(),
    ))
}

pub async fn get_location(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<LocationResponse>, ApiError> {
    rbac::require(&ctx, "lookups.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let row = repo::get_location(db.get_pool(), ctx.organization_id(), id).await?;
    Ok(ApiResponse::success(location_response(row, &locale)))
}

pub async fn create_location(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Json(payload): Json<CreateLocationRequest>,
) -> Result<ApiResponse<LocationResponse>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    validate_translations(&payload.translations)?;
    if let Some(location_type) = payload.location_type.as_deref() {
        validate_enum("location_type", location_type, &LOCATION_TYPES)?;
    }
    validate_country_code(payload.country_code.as_deref())?;
    if let Some(code) = payload.code.as_deref() {
        validate_code(&code.to_uppercase(), 16, true, "code")?;
    }
    let row = repo::create_location(db.get_pool(), ctx.organization_id(), &payload).await?;
    Ok(ApiResponse::success_with_message(
        location_response(row, &locale),
        "Location created",
    ))
}

pub async fn update_location(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateLocationRequest>,
) -> Result<ApiResponse<LocationResponse>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    if let Some(translations) = &payload.translations {
        validate_translations(translations)?;
    }
    if let Some(location_type) = payload.location_type.as_deref() {
        validate_enum("location_type", location_type, &LOCATION_TYPES)?;
    }
    if let Some(Some(country)) = payload.country_code.as_ref() {
        validate_country_code(Some(country))?;
    }
    let row = repo::update_location(db.get_pool(), ctx.organization_id(), id, &payload).await?;
    Ok(ApiResponse::success(location_response(row, &locale)))
}

pub async fn delete_location(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<String>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    repo::delete_location(db.get_pool(), ctx.organization_id(), id).await?;
    Ok(ApiResponse::success("Location deleted".to_string()))
}

// -------------------------------------------------------------- suppliers ----

fn supplier_response(row: repo::SupplierRow, locale: &str) -> SupplierResponse {
    SupplierResponse {
        name: i18n::pick(&row.translations, locale),
        id: row.id,
        translations: row.translations,
        contact_name: row.contact_name,
        email: row.email,
        phone_number: row.phone_number,
        address: row.address,
        url: row.url,
        notes: row.notes,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

pub async fn list_suppliers(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Query(query): Query<ListQuery>,
) -> Result<ApiResponse<Vec<SupplierResponse>>, ApiError> {
    rbac::require(&ctx, "lookups.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let rows =
        repo::list_suppliers(db.get_pool(), ctx.organization_id(), query.q.as_deref()).await?;
    Ok(ApiResponse::success(
        rows.into_iter()
            .map(|row| supplier_response(row, &locale))
            .collect(),
    ))
}

pub async fn get_supplier(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<SupplierResponse>, ApiError> {
    rbac::require(&ctx, "lookups.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let row = repo::get_supplier(db.get_pool(), ctx.organization_id(), id).await?;
    Ok(ApiResponse::success(supplier_response(row, &locale)))
}

pub async fn create_supplier(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Json(payload): Json<CreateSupplierRequest>,
) -> Result<ApiResponse<SupplierResponse>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    validate_translations(&payload.translations)?;
    let row = repo::create_supplier(db.get_pool(), ctx.organization_id(), &payload).await?;
    Ok(ApiResponse::success_with_message(
        supplier_response(row, &locale),
        "Supplier created",
    ))
}

pub async fn update_supplier(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateSupplierRequest>,
) -> Result<ApiResponse<SupplierResponse>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    if let Some(translations) = &payload.translations {
        validate_translations(translations)?;
    }
    let row = repo::update_supplier(db.get_pool(), ctx.organization_id(), id, &payload).await?;
    Ok(ApiResponse::success(supplier_response(row, &locale)))
}

pub async fn delete_supplier(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<String>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    repo::delete_supplier(db.get_pool(), ctx.organization_id(), id).await?;
    Ok(ApiResponse::success("Supplier deleted".to_string()))
}

// --------------------------------------------------- depreciation models ----

fn depreciation_response(
    row: repo::DepreciationModelRow,
    locale: &str,
) -> DepreciationModelResponse {
    DepreciationModelResponse {
        name: i18n::pick(&row.translations, locale),
        id: row.id,
        translations: row.translations,
        months: row.months,
        floor_value: row.floor_value,
        notes: row.notes,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

pub async fn list_depreciation_models(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Query(query): Query<ListQuery>,
) -> Result<ApiResponse<Vec<DepreciationModelResponse>>, ApiError> {
    rbac::require(&ctx, "lookups.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let rows =
        repo::list_depreciation_models(db.get_pool(), ctx.organization_id(), query.q.as_deref())
            .await?;
    Ok(ApiResponse::success(
        rows.into_iter()
            .map(|row| depreciation_response(row, &locale))
            .collect(),
    ))
}

pub async fn get_depreciation_model(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<DepreciationModelResponse>, ApiError> {
    rbac::require(&ctx, "lookups.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let row = repo::get_depreciation_model(db.get_pool(), ctx.organization_id(), id).await?;
    Ok(ApiResponse::success(depreciation_response(row, &locale)))
}

pub async fn create_depreciation_model(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Json(payload): Json<CreateDepreciationModelRequest>,
) -> Result<ApiResponse<DepreciationModelResponse>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    validate_translations(&payload.translations)?;
    if payload.months <= 0 {
        return Err(ApiError::bad_request(
            "months",
            "invalid",
            "months must be greater than zero",
        ));
    }
    let row =
        repo::create_depreciation_model(db.get_pool(), ctx.organization_id(), &payload).await?;
    Ok(ApiResponse::success_with_message(
        depreciation_response(row, &locale),
        "Depreciation model created",
    ))
}

pub async fn update_depreciation_model(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateDepreciationModelRequest>,
) -> Result<ApiResponse<DepreciationModelResponse>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    if let Some(translations) = &payload.translations {
        validate_translations(translations)?;
    }
    if let Some(months) = payload.months
        && months <= 0
    {
        return Err(ApiError::bad_request(
            "months",
            "invalid",
            "months must be greater than zero",
        ));
    }
    let row =
        repo::update_depreciation_model(db.get_pool(), ctx.organization_id(), id, &payload).await?;
    Ok(ApiResponse::success(depreciation_response(row, &locale)))
}

pub async fn delete_depreciation_model(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<String>, ApiError> {
    rbac::require(&ctx, "lookups.write")?;
    repo::delete_depreciation_model(db.get_pool(), ctx.organization_id(), id).await?;
    Ok(ApiResponse::success(
        "Depreciation model deleted".to_string(),
    ))
}
