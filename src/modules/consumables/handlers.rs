// Consumable handlers: catalog CRUD, low-stock feed and atomic stock moves.

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
use crate::modules::consumables::dto::*;
use crate::modules::consumables::repo;
use crate::modules::i18n::{self, Locale};
use crate::response::api_response::ApiResponse;

pub const TRANSACTION_TYPES: [&str; 3] = ["restock", "issue", "adjustment"];

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

pub fn consumable_response(row: repo::ConsumableRow, locale: &str) -> ConsumableResponse {
    ConsumableResponse {
        name: i18n::pick(&row.translations, locale),
        id: row.id,
        translations: row.translations,
        category_id: row.category_id,
        category_name: row.category_name,
        manufacturer_id: row.manufacturer_id,
        manufacturer_name: row.manufacturer_name,
        supplier_id: row.supplier_id,
        supplier_name: row.supplier_name,
        location_id: row.location_id,
        location_name: row.location_name,
        model_number: row.model_number,
        quantity_on_hand: row.quantity_on_hand,
        min_stock_alert: row.min_stock_alert,
        is_low_stock: row.is_low_stock,
        purchase_cost: row.purchase_cost,
        currency: row.currency,
        order_number: row.order_number,
        notes: row.notes,
        image_url: row.image_url,
        is_archived: row.is_archived,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

pub fn transaction_response(row: repo::TransactionRow) -> ConsumableTransactionResponse {
    ConsumableTransactionResponse {
        id: row.id,
        consumable_id: row.consumable_id,
        consumable_name: row.consumable_name,
        transaction_type: row.transaction_type,
        quantity: row.quantity,
        user_id: row.user_id,
        user_name: row.user_name,
        notes: row.notes,
        performed_by_user_id: row.performed_by_user_id,
        performed_by_user_name: row.performed_by_user_name,
        created_at: row.created_at,
    }
}

pub async fn list_consumables(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Query(query): Query<ConsumableListQuery>,
) -> Result<ApiResponse<Vec<ConsumableResponse>>, ApiError> {
    rbac::require(&ctx, "consumables.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(25).clamp(1, 100);

    let (rows, total) = repo::list_consumables(
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
            .map(|row| consumable_response(row, &locale))
            .collect(),
        page,
        per_page,
        total as u64,
    ))
}

pub async fn low_stock_alerts(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
) -> Result<ApiResponse<Vec<ConsumableResponse>>, ApiError> {
    rbac::require(&ctx, "consumables.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let rows = repo::low_stock_alerts(db.get_pool(), ctx.organization_id(), &locale).await?;
    Ok(ApiResponse::success(
        rows.into_iter()
            .map(|row| consumable_response(row, &locale))
            .collect(),
    ))
}

pub async fn get_consumable(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<ConsumableResponse>, ApiError> {
    rbac::require(&ctx, "consumables.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let row = repo::get_consumable(db.get_pool(), ctx.organization_id(), &locale, id).await?;
    Ok(ApiResponse::success(consumable_response(row, &locale)))
}

pub async fn create_consumable(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    headers: HeaderMap,
    Json(payload): Json<CreateConsumableRequest>,
) -> Result<ApiResponse<ConsumableResponse>, ApiError> {
    rbac::require(&ctx, "consumables.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    validate_translations(&payload.translations)?;
    validate_currency(payload.currency.as_deref())?;
    if payload.quantity_on_hand < 0 || payload.min_stock_alert < 0 {
        return Err(ApiError::bad_request(
            "quantity_on_hand",
            "invalid",
            "quantities cannot be negative",
        ));
    }
    let row = repo::create_consumable(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        ctx.user.id,
        &client_ip(&headers),
        &payload,
    )
    .await?;
    Ok(ApiResponse::success_with_message(
        consumable_response(row, &locale),
        "Consumable created",
    ))
}

pub async fn update_consumable(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateConsumableRequest>,
) -> Result<ApiResponse<ConsumableResponse>, ApiError> {
    rbac::require(&ctx, "consumables.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    if let Some(translations) = &payload.translations {
        validate_translations(translations)?;
    }
    validate_currency(payload.currency.as_deref())?;
    if payload.min_stock_alert.is_some_and(|q| q < 0) {
        return Err(ApiError::bad_request(
            "min_stock_alert",
            "invalid",
            "min_stock_alert cannot be negative",
        ));
    }
    let row = repo::update_consumable(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        ctx.user.id,
        &client_ip(&headers),
        id,
        &payload,
    )
    .await?;
    Ok(ApiResponse::success(consumable_response(row, &locale)))
}

pub async fn delete_consumable(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<String>, ApiError> {
    rbac::require(&ctx, "consumables.write")?;
    repo::delete_consumable(
        db.get_pool(),
        ctx.organization_id(),
        ctx.user.id,
        &client_ip(&headers),
        id,
    )
    .await?;
    Ok(ApiResponse::success("Consumable deleted".to_string()))
}

pub async fn list_transactions(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<Vec<ConsumableTransactionResponse>>, ApiError> {
    rbac::require(&ctx, "consumables.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let rows = repo::list_transactions(db.get_pool(), ctx.organization_id(), &locale, id).await?;
    Ok(ApiResponse::success(
        rows.into_iter().map(transaction_response).collect(),
    ))
}

pub async fn create_transaction(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<CreateConsumableTransactionRequest>,
) -> Result<ApiResponse<ConsumableTransactionResponse>, ApiError> {
    rbac::require(&ctx, "consumables.issue")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    validate_enum(
        "transaction_type",
        &payload.transaction_type,
        &TRANSACTION_TYPES,
    )?;
    if payload.transaction_type == "adjustment" {
        if payload.quantity == 0 {
            return Err(ApiError::bad_request(
                "quantity",
                "invalid",
                "adjustment quantity cannot be zero",
            ));
        }
    } else if payload.quantity <= 0 {
        return Err(ApiError::bad_request(
            "quantity",
            "invalid",
            "quantity must be greater than zero",
        ));
    }

    let row = repo::create_transaction(
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
        transaction_response(row),
        "Stock updated",
    ))
}
