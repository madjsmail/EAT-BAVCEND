// Asset handlers: CRUD, quick-scan, custody transitions and label rendering.

use std::io::Cursor;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use barcoders::sym::code128::Code128;
use qrcode::QrCode;
use serde::Deserialize;
use uuid::Uuid;

use crate::config::database::{Database, DatabaseTrait};
use crate::error::api_error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::{client_ip, rbac};
use crate::modules::accessories;
use crate::modules::assets::dto::*;
use crate::modules::assets::repo;
use crate::modules::consumables;
use crate::modules::i18n::Locale;
use crate::modules::licenses;
use crate::response::api_response::ApiResponse;

pub fn assignment_response(row: repo::AssignmentRow) -> AssignmentResponse {
    AssignmentResponse {
        id: row.id,
        asset_id: row.asset_id,
        action: row.action,
        assigned_to_user_id: row.assigned_to_user_id,
        assigned_to_user_name: row.assigned_to_user_name,
        from_user_id: row.from_user_id,
        from_user_name: row.from_user_name,
        from_location_id: row.from_location_id,
        from_location_name: row.from_location_name,
        to_location_id: row.to_location_id,
        to_location_name: row.to_location_name,
        from_department_id: row.from_department_id,
        to_department_id: row.to_department_id,
        from_status_id: row.from_status_id,
        to_status_id: row.to_status_id,
        notes: row.notes,
        performed_by_user_id: row.performed_by_user_id,
        performed_by_user_name: row.performed_by_user_name,
        created_at: row.created_at,
    }
}

fn asset_response(row: repo::AssetRow) -> AssetResponse {
    AssetResponse {
        id: row.id,
        asset_tag: row.asset_tag,
        name: row.name,
        product_id: row.product_id,
        product_name: row.product_name,
        product_code: Some(row.product_code),
        category_id: row.category_id,
        category_name: row.category_name,
        manufacturer_name: row.manufacturer_name,
        status_id: row.status_id,
        status_name: row.status_name,
        status_type: row.status_type,
        status_color_code: row.status_color_code,
        supplier_id: row.supplier_id,
        supplier_name: row.supplier_name,
        location_id: row.location_id,
        location_name: row.location_name,
        department_id: row.department_id,
        department_name: row.department_name,
        assigned_user_id: row.assigned_user_id,
        assigned_user_name: row.assigned_user_name,
        serial_number: row.serial_number,
        warranty_date: row.warranty_date,
        order_number: row.order_number,
        purchase_date: row.purchase_date,
        purchase_cost: row.purchase_cost,
        currency: row.currency,
        schedule_audit: row.schedule_audit,
        notes: row.notes,
        image_url: row.image_url,
        imei_1: row.imei_1,
        imei_2: row.imei_2,
        mdm_enrolled: row.mdm_enrolled,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
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

fn validate_asset_tag(tag: &str) -> Result<(), ApiError> {
    let ok = (1..=64).contains(&tag.len())
        && tag.bytes().all(|b| {
            b.is_ascii_uppercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'_')
        });
    if ok {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "asset_tag",
            "invalid",
            "asset_tag must be 1-64 chars of A-Z, 0-9, '.', '-' or '_'",
        ))
    }
}

pub async fn list_assets(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Query(query): Query<AssetListQuery>,
) -> Result<ApiResponse<Vec<AssetResponse>>, ApiError> {
    rbac::require(&ctx, "assets.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(25).clamp(1, 100);

    let (rows, total) = repo::list_assets(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        &query,
        page,
        per_page,
    )
    .await?;

    Ok(ApiResponse::paginated(
        rows.into_iter().map(asset_response).collect(),
        page,
        per_page,
        total as u64,
    ))
}

pub async fn get_asset(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<AssetResponse>, ApiError> {
    rbac::require(&ctx, "assets.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let row = repo::get_asset(db.get_pool(), ctx.organization_id(), &locale, id).await?;
    Ok(ApiResponse::success(asset_response(row)))
}

#[derive(Debug, Deserialize)]
pub struct ScanQuery {
    pub code: String,
}

pub async fn scan_asset(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Query(query): Query<ScanQuery>,
) -> Result<ApiResponse<AssetResponse>, ApiError> {
    rbac::require(&ctx, "assets.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    if query.code.trim().is_empty() {
        return Err(ApiError::bad_request(
            "code",
            "required",
            "Provide an asset_tag or serial_number in ?code=",
        ));
    }
    let row = repo::scan_asset(db.get_pool(), ctx.organization_id(), &locale, &query.code).await?;
    Ok(ApiResponse::success(asset_response(row)))
}

pub async fn create_asset(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    headers: HeaderMap,
    Json(payload): Json<CreateAssetRequest>,
) -> Result<ApiResponse<AssetResponse>, ApiError> {
    rbac::require(&ctx, "assets.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    if payload.name.trim().is_empty() {
        return Err(ApiError::bad_request(
            "name",
            "required",
            "Asset name is required",
        ));
    }
    if let Some(tag) = payload.asset_tag.as_deref() {
        validate_asset_tag(&tag.to_uppercase())?;
    }
    validate_currency(payload.currency.as_deref())?;
    if payload
        .purchase_cost
        .is_some_and(|c| c < rust_decimal::Decimal::ZERO)
    {
        return Err(ApiError::bad_request(
            "purchase_cost",
            "invalid",
            "purchase_cost cannot be negative",
        ));
    }

    let row = repo::create_asset(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        ctx.user.id,
        client_ip(&headers),
        &payload,
    )
    .await?;
    Ok(ApiResponse::success_with_message(
        asset_response(row),
        "Asset created",
    ))
}

pub async fn update_asset(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateAssetRequest>,
) -> Result<ApiResponse<AssetResponse>, ApiError> {
    rbac::require(&ctx, "assets.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    if let Some(name) = payload.name.as_deref()
        && name.trim().is_empty()
    {
        return Err(ApiError::bad_request(
            "name",
            "invalid",
            "Asset name cannot be blank",
        ));
    }
    if let Some(tag) = payload.asset_tag.as_deref() {
        validate_asset_tag(&tag.to_uppercase())?;
    }
    validate_currency(payload.currency.as_deref())?;

    let row = repo::update_asset(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        ctx.user.id,
        client_ip(&headers),
        id,
        &payload,
    )
    .await?;
    Ok(ApiResponse::success(asset_response(row)))
}

pub async fn delete_asset(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<String>, ApiError> {
    rbac::require(&ctx, "assets.write")?;
    repo::delete_asset(
        db.get_pool(),
        ctx.organization_id(),
        ctx.user.id,
        client_ip(&headers),
        id,
    )
    .await?;
    Ok(ApiResponse::success("Asset deleted".to_string()))
}

// --------------------------------------------------------------- custody ----

pub async fn assign_asset(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<AssignAssetRequest>,
) -> Result<ApiResponse<AssetResponse>, ApiError> {
    rbac::require(&ctx, "assets.assign")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let row = repo::assign_asset(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        ctx.user.id,
        client_ip(&headers),
        id,
        &payload,
    )
    .await?;
    Ok(ApiResponse::success_with_message(
        asset_response(row),
        "Asset assigned",
    ))
}

pub async fn return_asset(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<ReturnAssetRequest>,
) -> Result<ApiResponse<AssetResponse>, ApiError> {
    rbac::require(&ctx, "assets.assign")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let row = repo::return_asset(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        ctx.user.id,
        client_ip(&headers),
        id,
        &payload,
    )
    .await?;
    Ok(ApiResponse::success_with_message(
        asset_response(row),
        "Asset returned to stock",
    ))
}

pub async fn transfer_asset(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<TransferAssetRequest>,
) -> Result<ApiResponse<AssetResponse>, ApiError> {
    rbac::require(&ctx, "assets.assign")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let row = repo::transfer_asset(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        ctx.user.id,
        client_ip(&headers),
        id,
        &payload,
    )
    .await?;
    Ok(ApiResponse::success_with_message(
        asset_response(row),
        "Asset transferred",
    ))
}

pub async fn list_assignments(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<Vec<AssignmentResponse>>, ApiError> {
    rbac::require(&ctx, "assets.read")?;
    let rows = repo::list_assignments(db.get_pool(), ctx.organization_id(), id).await?;
    Ok(ApiResponse::success(
        rows.into_iter().map(assignment_response).collect(),
    ))
}

/// One round trip per pillar: everything the asset detail page needs.
pub async fn asset_360(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<Asset360Response>, ApiError> {
    rbac::require(&ctx, "assets.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let org = ctx.organization_id();
    let pool = db.get_pool();

    let asset = repo::get_asset(pool, org, &locale, id).await?;

    let (accessory_rows, asset_seats, custody_rows) = tokio::try_join!(
        accessories::repo::list_active_for_asset(pool, org, &locale, id),
        licenses::repo::list_active_for_asset(pool, org, &locale, id),
        repo::list_assignments(pool, org, id),
    )?;

    // Consumables are issued to people, so they follow the asset's holder.
    let (consumable_rows, user_seats) = match asset.assigned_user_id {
        Some(user_id) => tokio::try_join!(
            consumables::repo::list_issued_to_user(pool, org, &locale, user_id),
            licenses::repo::list_active_for_user(pool, org, &locale, user_id),
        )?,
        None => (Vec::new(), Vec::new()),
    };

    let accessories = accessory_rows
        .into_iter()
        .map(accessories::handlers::assignment_response)
        .collect::<Vec<_>>();
    let consumables = consumable_rows
        .into_iter()
        .map(consumables::handlers::transaction_response)
        .collect::<Vec<_>>();
    let licenses = asset_seats
        .into_iter()
        .chain(user_seats)
        .map(licenses::handlers::seat_response)
        .collect::<Vec<_>>();
    let custody_history = custody_rows
        .into_iter()
        .map(assignment_response)
        .collect::<Vec<_>>();

    let counts = Asset360Counts {
        accessories: accessories.len(),
        consumables: consumables.len(),
        licenses: licenses.len(),
        custody_events: custody_history.len(),
    };

    Ok(ApiResponse::success(Asset360Response {
        asset: asset_response(asset),
        accessories,
        consumables,
        licenses,
        custody_history,
        counts,
    }))
}

// ---------------------------------------------------------------- labels ----

#[derive(Debug, Deserialize)]
pub struct LabelQuery {
    /// `qr` (default) or `code128`.
    pub symbology: Option<String>,
    /// `json` (default), `svg` or `png`.
    pub format: Option<String>,
    /// `asset_tag` (default) or `serial_number`.
    pub data: Option<String>,
}

pub async fn asset_label(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
    Query(query): Query<LabelQuery>,
) -> Result<Response, ApiError> {
    rbac::require(&ctx, "assets.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);

    let symbology = query.symbology.unwrap_or_else(|| "qr".to_string());
    let format = query.format.unwrap_or_else(|| "json".to_string());
    let data_source = query.data.unwrap_or_else(|| "asset_tag".to_string());

    if !matches!(symbology.as_str(), "qr" | "code128") {
        return Err(ApiError::bad_request(
            "symbology",
            "invalid",
            "symbology must be qr or code128",
        ));
    }
    if !matches!(format.as_str(), "json" | "svg" | "png") {
        return Err(ApiError::bad_request(
            "format",
            "invalid",
            "format must be json, svg or png",
        ));
    }
    if !matches!(data_source.as_str(), "asset_tag" | "serial_number") {
        return Err(ApiError::bad_request(
            "data",
            "invalid",
            "data must be asset_tag or serial_number",
        ));
    }

    let row = repo::get_asset(db.get_pool(), ctx.organization_id(), &locale, id).await?;
    let data = match data_source.as_str() {
        "serial_number" => row.serial_number.clone().ok_or_else(|| {
            ApiError::bad_request(
                "data",
                "missing",
                "This asset has no serial_number; use data=asset_tag",
            )
        })?,
        _ => row.asset_tag.clone(),
    };

    let payload = LabelPayload {
        asset_id: row.id,
        asset_tag: row.asset_tag.clone(),
        serial_number: row.serial_number.clone(),
        name: row.name.clone(),
        status_name: row.status_name.clone(),
        location_name: row.location_name.clone(),
        department_name: row.department_name.clone(),
        assigned_user_name: row.assigned_user_name.clone(),
        data: data.clone(),
        data_source,
        symbology: symbology.clone(),
    };

    if format == "json" {
        return Ok(ApiResponse::success(payload).into_response());
    }

    let (bytes, content_type) = if symbology == "qr" {
        if format == "svg" {
            (
                render_qr_svg(&data)?.into_bytes(),
                "image/svg+xml; charset=utf-8",
            )
        } else {
            (render_qr_png(&data)?, "image/png")
        }
    } else if format == "svg" {
        (
            render_code128_svg(&data)?.into_bytes(),
            "image/svg+xml; charset=utf-8",
        )
    } else {
        (render_code128_png(&data)?, "image/png")
    };

    let mut response = (StatusCode::OK, bytes).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static(content_type),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&format!(
            "inline; filename=\"{}.{}\"",
            payload.asset_tag.replace('.', "-"),
            if format == "svg" { "svg" } else { "png" }
        ))
        .map_err(|e| ApiError::Internal(e.to_string()))?,
    );
    Ok(response)
}

// Code128 module width / bar height in pixels (600dpi-ish label proportions).
const BARCODE_SCALE: u32 = 2;
const BARCODE_HEIGHT: u32 = 80;
const QUIET_ZONE: u32 = 12;

fn code128_modules(data: &str) -> Result<Vec<u8>, ApiError> {
    // barcoders requires an explicit initial character set; B covers printable ASCII.
    let payload = format!("\u{0181}{data}");
    Code128::new(&payload)
        .map(|code| code.encode())
        .map_err(|e| ApiError::bad_request("data", "unencodable", format!("Code128: {e:?}")))
}

fn render_qr_svg(data: &str) -> Result<String, ApiError> {
    let code = QrCode::new(data.as_bytes())
        .map_err(|e| ApiError::bad_request("data", "unencodable", format!("QR: {e}")))?;
    Ok(code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(256, 256)
        .quiet_zone(true)
        .dark_color(qrcode::render::svg::Color("#000000"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build())
}

fn render_qr_png(data: &str) -> Result<Vec<u8>, ApiError> {
    let code = QrCode::new(data.as_bytes())
        .map_err(|e| ApiError::bad_request("data", "unencodable", format!("QR: {e}")))?;
    let image = code
        .render::<image::Luma<u8>>()
        .min_dimensions(256, 256)
        .quiet_zone(true)
        .build();
    let mut buffer = Vec::new();
    image::DynamicImage::ImageLuma8(image)
        .write_to(&mut Cursor::new(&mut buffer), image::ImageFormat::Png)
        .map_err(|e| ApiError::Internal(format!("PNG encode failed: {e}")))?;
    Ok(buffer)
}

fn render_code128_svg(data: &str) -> Result<String, ApiError> {
    let modules = code128_modules(data)?;
    let width = modules.len() as u32 * BARCODE_SCALE + QUIET_ZONE * 2;
    let height = BARCODE_HEIGHT + QUIET_ZONE * 2;

    let mut svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}"><rect width="100%" height="100%" fill="#ffffff"/>"##
    );
    for (index, module) in modules.iter().enumerate() {
        if *module == 1 {
            let x = QUIET_ZONE + index as u32 * BARCODE_SCALE;
            svg.push_str(&format!(
                r##"<rect x="{x}" y="{QUIET_ZONE}" width="{BARCODE_SCALE}" height="{BARCODE_HEIGHT}" fill="#000000"/>"##
            ));
        }
    }
    svg.push_str("</svg>");
    Ok(svg)
}

fn render_code128_png(data: &str) -> Result<Vec<u8>, ApiError> {
    let modules = code128_modules(data)?;
    let width = modules.len() as u32 * BARCODE_SCALE + QUIET_ZONE * 2;
    let height = BARCODE_HEIGHT + QUIET_ZONE * 2;
    let mut image = image::RgbImage::from_pixel(width, height, image::Rgb([255u8, 255u8, 255u8]));

    for (index, module) in modules.iter().enumerate() {
        if *module == 1 {
            let start = QUIET_ZONE + index as u32 * BARCODE_SCALE;
            for x in start..start + BARCODE_SCALE {
                for y in QUIET_ZONE..QUIET_ZONE + BARCODE_HEIGHT {
                    image.put_pixel(x, y, image::Rgb([0u8, 0u8, 0u8]));
                }
            }
        }
    }

    let mut buffer = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut buffer), image::ImageFormat::Png)
        .map_err(|e| ApiError::Internal(format!("PNG encode failed: {e}")))?;
    Ok(buffer)
}
