// Product catalog handlers: validate -> RBAC -> repo -> JSON.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use uuid::Uuid;

use crate::config::database::{Database, DatabaseTrait};
use crate::error::api_error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::rbac;
use crate::modules::common::validate_enum;
use crate::modules::i18n::{self, Locale};
use crate::modules::products::dto::*;
use crate::modules::products::repo;
use crate::response::api_response::ApiResponse;

pub const EOL_TYPES: [&str; 4] = [
    "fixed_date",
    "months_from_purchase",
    "months_from_warranty",
    "individual",
];

fn product_response(row: repo::ProductRow, locale: &str) -> ProductResponse {
    ProductResponse {
        name: i18n::pick(&row.translations, locale),
        id: row.id,
        code: row.code,
        translations: row.translations,
        category_id: row.category_id,
        category_name: row.category_name,
        manufacturer_id: row.manufacturer_id,
        manufacturer_name: row.manufacturer_name,
        depreciation_model_id: row.depreciation_model_id,
        depreciation_model_name: row.depreciation_model_name,
        model_code: row.model_code,
        eol_type: row.eol_type,
        eol_date: row.eol_date,
        default_purchase_cost: row.default_purchase_cost,
        default_currency: row.default_currency,
        default_supplier_id: row.default_supplier_id,
        default_supplier_name: row.default_supplier_name,
        min_quantity: row.min_quantity,
        chip: row.chip,
        operating_system: row.operating_system,
        notes: row.notes,
        custom_fields_json: row.custom_fields_json,
        specs_json: row.specs_json,
        is_archived: row.is_archived,
        image_url: row.image_url,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn validate_translations(translations: &i18n::Translations) -> Result<(), ApiError> {
    i18n::validate(translations)
        .map_err(|message| ApiError::bad_request("translations", "invalid", message))
}

fn validate_currency(code: Option<&str>) -> Result<(), ApiError> {
    match code {
        Some(code) if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_alphabetic()) => {
            Err(ApiError::bad_request(
                "default_currency",
                "invalid",
                "default_currency must be a 3-letter ISO-4217 code",
            ))
        }
        _ => Ok(()),
    }
}

pub async fn list_products(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Query(query): Query<ProductListQuery>,
) -> Result<ApiResponse<Vec<ProductResponse>>, ApiError> {
    rbac::require(&ctx, "products.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(25).clamp(1, 100);

    let (rows, total) = repo::list_products(
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
            .map(|row| product_response(row, &locale))
            .collect(),
        page,
        per_page,
        total as u64,
    ))
}

pub async fn get_product(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<ProductResponse>, ApiError> {
    rbac::require(&ctx, "products.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let row = repo::get_product(db.get_pool(), ctx.organization_id(), &locale, id).await?;
    Ok(ApiResponse::success(product_response(row, &locale)))
}

pub async fn create_product(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Json(payload): Json<CreateProductRequest>,
) -> Result<ApiResponse<ProductResponse>, ApiError> {
    rbac::require(&ctx, "products.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    validate_translations(&payload.translations)?;
    if let Some(eol_type) = payload.eol_type.as_deref() {
        validate_enum("eol_type", eol_type, &EOL_TYPES)?;
        if eol_type == "fixed_date" && payload.eol_date.is_none() {
            return Err(ApiError::bad_request(
                "eol_date",
                "required",
                "eol_date is required when eol_type is fixed_date",
            ));
        }
    }
    validate_currency(payload.default_currency.as_deref())?;
    if payload.min_quantity.is_some_and(|q| q < 0) {
        return Err(ApiError::bad_request(
            "min_quantity",
            "invalid",
            "min_quantity cannot be negative",
        ));
    }

    let row = repo::create_product(db.get_pool(), ctx.organization_id(), &locale, &payload).await?;
    Ok(ApiResponse::success_with_message(
        product_response(row, &locale),
        "Product created",
    ))
}

pub async fn update_product(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateProductRequest>,
) -> Result<ApiResponse<ProductResponse>, ApiError> {
    rbac::require(&ctx, "products.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    if let Some(translations) = &payload.translations {
        validate_translations(translations)?;
    }
    if let Some(eol_type) = payload.eol_type.as_deref() {
        validate_enum("eol_type", eol_type, &EOL_TYPES)?;
    }
    validate_currency(payload.default_currency.as_deref())?;
    if payload.min_quantity.is_some_and(|q| q < 0) {
        return Err(ApiError::bad_request(
            "min_quantity",
            "invalid",
            "min_quantity cannot be negative",
        ));
    }

    let row =
        repo::update_product(db.get_pool(), ctx.organization_id(), &locale, id, &payload).await?;
    Ok(ApiResponse::success(product_response(row, &locale)))
}

pub async fn delete_product(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<String>, ApiError> {
    rbac::require(&ctx, "products.write")?;
    repo::delete_product(db.get_pool(), ctx.organization_id(), id).await?;
    Ok(ApiResponse::success("Product deleted".to_string()))
}
