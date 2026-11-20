// License handlers: catalog, seat allocation/revocation and expiry alerts.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use uuid::Uuid;

use crate::config::database::{Database, DatabaseTrait};
use crate::error::api_error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::{client_ip, rbac};
use crate::modules::common::validate_enum;
use crate::modules::i18n::{self, Locale};
use crate::modules::licenses::dto::*;
use crate::modules::licenses::repo;
use crate::response::api_response::ApiResponse;

pub const LICENSE_TYPES: [&str; 4] = ["named", "concurrent", "subscription", "perpetual"];

fn validate_translations(translations: &i18n::Translations) -> Result<(), ApiError> {
    i18n::validate(translations).map_err(|m| ApiError::bad_request("translations", "invalid", m))
}

fn validate_currency(currency: Option<&str>) -> Result<(), ApiError> {
    match currency {
        Some(code) if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_alphabetic()) => {
            Err(ApiError::bad_request(
                "currency",
                "invalid",
                "currency must be a 3-letter ISO-4217 code",
            ))
        }
        _ => Ok(()),
    }
}

fn license_response(row: repo::LicenseRow, locale: &str) -> LicenseResponse {
    LicenseResponse {
        name: i18n::pick(&row.translations, locale),
        id: row.id,
        translations: row.translations,
        manufacturer_id: row.manufacturer_id,
        manufacturer_name: row.manufacturer_name,
        supplier_id: row.supplier_id,
        supplier_name: row.supplier_name,
        license_type: row.license_type,
        total_seats: row.total_seats,
        active_seats: row.active_seats,
        available_seats: row.available_seats,
        purchase_date: row.purchase_date,
        expiration_date: row.expiration_date,
        days_until_expiry: row.days_until_expiry,
        expiry_bucket: row.expiry_bucket,
        purchase_cost: row.purchase_cost,
        currency: row.currency,
        order_number: row.order_number,
        product_key: row.product_key,
        notes: row.notes,
        is_archived: row.is_archived,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

pub fn seat_response(row: repo::SeatRow) -> LicenseSeatResponse {
    LicenseSeatResponse {
        id: row.id,
        license_id: row.license_id,
        license_name: row.license_name,
        user_id: row.user_id,
        user_name: row.user_name,
        asset_id: row.asset_id,
        asset_tag: row.asset_tag,
        notes: row.notes,
        assigned_by_user_id: row.assigned_by_user_id,
        assigned_by_user_name: row.assigned_by_user_name,
        revoked_by_user_id: row.revoked_by_user_id,
        revoked_by_user_name: row.revoked_by_user_name,
        created_at: row.created_at,
        revoked_at: row.revoked_at,
    }
}

pub async fn list_licenses(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Query(query): Query<LicenseListQuery>,
) -> Result<ApiResponse<Vec<LicenseResponse>>, ApiError> {
    rbac::require(&ctx, "licenses.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(25).clamp(1, 100);

    let (rows, total) = repo::list_licenses(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        &query,
        page,
        per_page,
    )
    .await?;
    Ok(ApiResponse::paginated(
        rows.into_iter()
            .map(|row| license_response(row, &locale))
            .collect(),
        page,
        per_page,
        total as u64,
    ))
}

pub async fn expiry_alerts(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
) -> Result<ApiResponse<ExpiryAlerts>, ApiError> {
    rbac::require(&ctx, "licenses.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let rows = repo::expiry_alerts(db.get_pool(), ctx.organization_id(), &locale).await?;

    let mut alerts = ExpiryAlerts {
        expired: Vec::new(),
        within_30_days: Vec::new(),
        within_60_days: Vec::new(),
        within_90_days: Vec::new(),
    };
    for row in rows {
        let days = row.days_until_expiry.unwrap_or(i32::MAX);
        let target = if days < 0 {
            &mut alerts.expired
        } else if days <= 30 {
            &mut alerts.within_30_days
        } else if days <= 60 {
            &mut alerts.within_60_days
        } else {
            &mut alerts.within_90_days
        };
        target.push(license_response(row, &locale));
    }
    Ok(ApiResponse::success(alerts))
}

pub async fn get_license(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<LicenseResponse>, ApiError> {
    rbac::require(&ctx, "licenses.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let row = repo::get_license(db.get_pool(), ctx.organization_id(), &locale, id).await?;
    Ok(ApiResponse::success(license_response(row, &locale)))
}

pub async fn create_license(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    headers: HeaderMap,
    Json(payload): Json<CreateLicenseRequest>,
) -> Result<ApiResponse<LicenseResponse>, ApiError> {
    rbac::require(&ctx, "licenses.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    validate_translations(&payload.translations)?;
    validate_currency(payload.currency.as_deref())?;
    if payload.total_seats < 0 {
        return Err(ApiError::bad_request(
            "total_seats",
            "invalid",
            "total_seats cannot be negative",
        ));
    }
    if let Some(license_type) = payload.license_type.as_deref() {
        validate_enum("license_type", license_type, &LICENSE_TYPES)?;
    }
    let row = repo::create_license(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        ctx.user.id,
        &client_ip(&headers),
        &payload,
    )
    .await?;
    Ok(ApiResponse::success_with_message(
        license_response(row, &locale),
        "License created",
    ))
}

pub async fn update_license(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateLicenseRequest>,
) -> Result<ApiResponse<LicenseResponse>, ApiError> {
    rbac::require(&ctx, "licenses.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    if let Some(translations) = &payload.translations {
        validate_translations(translations)?;
    }
    validate_currency(payload.currency.as_deref())?;
    if payload.total_seats.is_some_and(|seats| seats < 0) {
        return Err(ApiError::bad_request(
            "total_seats",
            "invalid",
            "total_seats cannot be negative",
        ));
    }
    if let Some(license_type) = payload.license_type.as_deref() {
        validate_enum("license_type", license_type, &LICENSE_TYPES)?;
    }
    let row = repo::update_license(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        ctx.user.id,
        &client_ip(&headers),
        id,
        &payload,
    )
    .await?;
    Ok(ApiResponse::success(license_response(row, &locale)))
}

pub async fn delete_license(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<String>, ApiError> {
    rbac::require(&ctx, "licenses.write")?;
    repo::delete_license(
        db.get_pool(),
        ctx.organization_id(),
        ctx.user.id,
        &client_ip(&headers),
        id,
    )
    .await?;
    Ok(ApiResponse::success("License deleted".to_string()))
}

pub async fn list_seats(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<Vec<LicenseSeatResponse>>, ApiError> {
    rbac::require(&ctx, "licenses.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let rows = repo::list_seats(db.get_pool(), ctx.organization_id(), &locale, id).await?;
    Ok(ApiResponse::success(
        rows.into_iter().map(seat_response).collect(),
    ))
}

pub async fn allocate_seat(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<AllocateSeatRequest>,
) -> Result<ApiResponse<LicenseSeatResponse>, ApiError> {
    rbac::require(&ctx, "licenses.assign")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let row = repo::allocate_seat(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        ctx.user.id,
        &client_ip(&headers),
        id,
        &payload,
    )
    .await?;
    Ok(ApiResponse::success_with_message(
        seat_response(row),
        "Seat allocated",
    ))
}

pub async fn revoke_seat(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    headers: HeaderMap,
    Path((id, seat_id)): Path<(Uuid, Uuid)>,
    payload: Option<Json<RevokeSeatRequest>>,
) -> Result<ApiResponse<LicenseSeatResponse>, ApiError> {
    rbac::require(&ctx, "licenses.assign")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let payload = payload.map(|Json(p)| p).unwrap_or_default();
    let row = repo::revoke_seat(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        ctx.user.id,
        &client_ip(&headers),
        id,
        seat_id,
        &payload,
    )
    .await?;
    Ok(ApiResponse::success_with_message(
        seat_response(row),
        "Seat revoked",
    ))
}
