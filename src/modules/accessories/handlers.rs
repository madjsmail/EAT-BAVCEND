// Accessory handlers: CRUD, assignment and partial returns.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use uuid::Uuid;

use crate::config::database::{Database, DatabaseTrait};
use crate::error::api_error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::{client_ip, rbac};
use crate::modules::accessories::dto::*;
use crate::modules::accessories::repo;
use crate::modules::i18n::{self, Locale};
use crate::response::api_response::ApiResponse;

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

fn accessory_response(row: repo::AccessoryRow, locale: &str) -> AccessoryResponse {
    AccessoryResponse {
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
        total_quantity: row.total_quantity,
        available_quantity: row.available_quantity,
        assigned_quantity: row.assigned_quantity,
        min_quantity: row.min_quantity,
        purchase_cost: row.purchase_cost,
        currency: row.currency,
        purchase_date: row.purchase_date,
        order_number: row.order_number,
        notes: row.notes,
        image_url: row.image_url,
        is_archived: row.is_archived,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

pub fn assignment_response(row: repo::AssignmentRow) -> AccessoryAssignmentResponse {
    AccessoryAssignmentResponse {
        id: row.id,
        accessory_id: row.accessory_id,
        accessory_name: row.accessory_name,
        assigned_to_user_id: row.assigned_to_user_id,
        assigned_to_user_name: row.assigned_to_user_name,
        asset_id: row.asset_id,
        asset_tag: row.asset_tag,
        quantity: row.quantity,
        returned_quantity: row.returned_quantity,
        remaining_quantity: row.remaining_quantity,
        notes: row.notes,
        assigned_by_user_id: row.assigned_by_user_id,
        assigned_by_user_name: row.assigned_by_user_name,
        returned_by_user_id: row.returned_by_user_id,
        returned_by_user_name: row.returned_by_user_name,
        created_at: row.created_at,
        returned_at: row.returned_at,
    }
}

pub async fn list_accessories(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Query(query): Query<AccessoryListQuery>,
) -> Result<ApiResponse<Vec<AccessoryResponse>>, ApiError> {
    rbac::require(&ctx, "accessories.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(25).clamp(1, 100);

    let (rows, total) = repo::list_accessories(
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
            .map(|row| accessory_response(row, &locale))
            .collect(),
        page,
        per_page,
        total as u64,
    ))
}

pub async fn get_accessory(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<AccessoryResponse>, ApiError> {
    rbac::require(&ctx, "accessories.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let row = repo::get_accessory(db.get_pool(), ctx.organization_id(), &locale, id).await?;
    Ok(ApiResponse::success(accessory_response(row, &locale)))
}

pub async fn create_accessory(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    headers: HeaderMap,
    Json(payload): Json<CreateAccessoryRequest>,
) -> Result<ApiResponse<AccessoryResponse>, ApiError> {
    rbac::require(&ctx, "accessories.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    validate_translations(&payload.translations)?;
    validate_currency(payload.currency.as_deref())?;
    if payload.total_quantity < 0 || payload.min_quantity < 0 {
        return Err(ApiError::bad_request(
            "total_quantity",
            "invalid",
            "quantities cannot be negative",
        ));
    }
    let row = repo::create_accessory(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        ctx.user.id,
        &client_ip(&headers),
        &payload,
    )
    .await?;
    Ok(ApiResponse::success_with_message(
        accessory_response(row, &locale),
        "Accessory created",
    ))
}

pub async fn update_accessory(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateAccessoryRequest>,
) -> Result<ApiResponse<AccessoryResponse>, ApiError> {
    rbac::require(&ctx, "accessories.write")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    if let Some(translations) = &payload.translations {
        validate_translations(translations)?;
    }
    validate_currency(payload.currency.as_deref())?;
    if payload.total_quantity.is_some_and(|q| q < 0) || payload.min_quantity.is_some_and(|q| q < 0)
    {
        return Err(ApiError::bad_request(
            "total_quantity",
            "invalid",
            "quantities cannot be negative",
        ));
    }
    let row = repo::update_accessory(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        ctx.user.id,
        &client_ip(&headers),
        id,
        &payload,
    )
    .await?;
    Ok(ApiResponse::success(accessory_response(row, &locale)))
}

pub async fn delete_accessory(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<String>, ApiError> {
    rbac::require(&ctx, "accessories.write")?;
    repo::delete_accessory(
        db.get_pool(),
        ctx.organization_id(),
        ctx.user.id,
        &client_ip(&headers),
        id,
    )
    .await?;
    Ok(ApiResponse::success("Accessory deleted".to_string()))
}

pub async fn list_assignments(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<Vec<AccessoryAssignmentResponse>>, ApiError> {
    rbac::require(&ctx, "accessories.read")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let rows = repo::list_assignments(db.get_pool(), ctx.organization_id(), &locale, id).await?;
    Ok(ApiResponse::success(
        rows.into_iter().map(assignment_response).collect(),
    ))
}

pub async fn assign_accessory(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<AssignAccessoryRequest>,
) -> Result<ApiResponse<AccessoryAssignmentResponse>, ApiError> {
    rbac::require(&ctx, "accessories.assign")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let row = repo::assign_accessory(
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
        assignment_response(row),
        "Accessory assigned",
    ))
}

pub async fn return_accessory(
    State(db): State<Arc<Database>>,
    ctx: AuthContext,
    locale: Locale,
    headers: HeaderMap,
    Path((id, assignment_id)): Path<(Uuid, Uuid)>,
    Json(payload): Json<ReturnAccessoryRequest>,
) -> Result<ApiResponse<AccessoryAssignmentResponse>, ApiError> {
    rbac::require(&ctx, "accessories.assign")?;
    let locale = locale.resolve(&ctx.user.preferred_locale);
    let row = repo::return_accessory(
        db.get_pool(),
        ctx.organization_id(),
        &locale,
        ctx.user.id,
        &client_ip(&headers),
        id,
        assignment_id,
        &payload,
    )
    .await?;
    Ok(ApiResponse::success_with_message(
        assignment_response(row),
        "Accessory returned",
    ))
}
