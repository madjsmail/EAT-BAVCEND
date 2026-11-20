// Product catalog data access. Every statement filters by `organization_id`.

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::api_error::ApiError;
use crate::modules::common::{derive_code, ensure_org_ref, next_available_code, normalize_code};
use crate::modules::i18n::Translations;
use crate::modules::products::dto::*;

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

#[derive(Debug, FromRow)]
pub struct ProductRow {
    pub id: Uuid,
    pub code: String,
    pub category_id: Uuid,
    pub category_name: Option<String>,
    pub manufacturer_id: Option<Uuid>,
    pub manufacturer_name: Option<String>,
    pub depreciation_model_id: Option<Uuid>,
    pub depreciation_model_name: Option<String>,
    pub model_code: Option<String>,
    pub eol_type: String,
    pub eol_date: Option<NaiveDate>,
    pub default_purchase_cost: Option<Decimal>,
    pub default_currency: String,
    pub default_supplier_id: Option<Uuid>,
    pub default_supplier_name: Option<String>,
    pub min_quantity: i32,
    pub chip: Option<String>,
    pub operating_system: Option<String>,
    pub notes: Option<String>,
    #[sqlx(json)]
    pub custom_fields_json: Value,
    #[sqlx(json)]
    pub specs_json: Value,
    pub is_archived: bool,
    pub image_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[sqlx(json)]
    pub translations: std::collections::BTreeMap<String, String>,
}

/// `$1` organization, `$2` locale. Localized names fall back to English.
const PRODUCT_SELECT: &str = r#"
    SELECT p.id, p.code, p.category_id,
           COALESCE((SELECT t.name FROM category_translations t
                     WHERE t.category_id = p.category_id AND t.locale = $2),
                    (SELECT t.name FROM category_translations t
                     WHERE t.category_id = p.category_id AND t.locale = 'en')) AS category_name,
           p.manufacturer_id,
           COALESCE((SELECT t.name FROM manufacturer_translations t
                     WHERE t.manufacturer_id = p.manufacturer_id AND t.locale = $2),
                    (SELECT t.name FROM manufacturer_translations t
                     WHERE t.manufacturer_id = p.manufacturer_id AND t.locale = 'en')) AS manufacturer_name,
           p.depreciation_model_id,
           COALESCE((SELECT t.name FROM depreciation_model_translations t
                     WHERE t.depreciation_model_id = p.depreciation_model_id AND t.locale = $2),
                    (SELECT t.name FROM depreciation_model_translations t
                     WHERE t.depreciation_model_id = p.depreciation_model_id AND t.locale = 'en')) AS depreciation_model_name,
           p.model_code, p.eol_type, p.eol_date, p.default_purchase_cost, p.default_currency,
           p.default_supplier_id,
           COALESCE((SELECT t.name FROM supplier_translations t
                     WHERE t.supplier_id = p.default_supplier_id AND t.locale = $2),
                    (SELECT t.name FROM supplier_translations t
                     WHERE t.supplier_id = p.default_supplier_id AND t.locale = 'en')) AS default_supplier_name,
           p.min_quantity, p.chip, p.operating_system, p.notes, p.custom_fields_json,
           p.specs_json, p.is_archived, p.image_url, p.created_at, p.updated_at,
           COALESCE((SELECT jsonb_object_agg(t.locale, t.name)
                     FROM product_translations t WHERE t.product_id = p.id), '{}'::jsonb) AS translations
    FROM products p
    WHERE p.organization_id = $1
"#;

/// `$3` q, `$4` category, `$5` manufacturer, `$6` archived.
const PRODUCT_FILTER: &str = r#"
      AND ($3::text IS NULL OR p.code ILIKE '%' || $3 || '%'
           OR p.model_code ILIKE '%' || $3 || '%'
           OR EXISTS (SELECT 1 FROM product_translations s
                      WHERE s.product_id = p.id AND s.name ILIKE '%' || $3 || '%'))
      AND ($4::uuid IS NULL OR p.category_id = $4)
      AND ($5::uuid IS NULL OR p.manufacturer_id = $5)
      AND ($6::boolean IS NULL OR p.is_archived = $6)
"#;

pub async fn list_products(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    query: &ProductListQuery,
    page: u32,
    per_page: u32,
) -> Result<(Vec<ProductRow>, i64)> {
    let sql = format!(
        "{PRODUCT_SELECT}{PRODUCT_FILTER} ORDER BY p.created_at DESC, p.id LIMIT $7 OFFSET $8"
    );
    let rows = sqlx::query_as::<_, ProductRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(query.q.as_deref())
        .bind(query.category_id)
        .bind(query.manufacturer_id)
        .bind(query.is_archived)
        .bind(per_page as i64)
        .bind(((page.saturating_sub(1)) * per_page) as i64)
        .fetch_all(pool)
        .await?;

    let count_sql =
        format!("SELECT count(*) FROM products p WHERE p.organization_id = $1{PRODUCT_FILTER}");
    let total: i64 = sqlx::query_scalar(&count_sql)
        .bind(org)
        .bind(locale)
        .bind(query.q.as_deref())
        .bind(query.category_id)
        .bind(query.manufacturer_id)
        .bind(query.is_archived)
        .fetch_one(pool)
        .await?;

    Ok((rows, total))
}

pub async fn get_product(pool: &PgPool, org: Uuid, locale: &str, id: Uuid) -> Result<ProductRow> {
    let sql = format!("{PRODUCT_SELECT} AND p.id = $3");
    sqlx::query_as::<_, ProductRow>(&sql)
        .bind(org)
        .bind(locale)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound("Product"))
}

/// Asset tag generation needs the product's own code plus its category code.
pub async fn product_tag_codes(
    conn: &mut sqlx::PgConnection,
    org: Uuid,
    product_id: Uuid,
) -> Result<(String, String)> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT p.code, c.code
         FROM products p JOIN categories c ON c.id = p.category_id
         WHERE p.id = $1 AND p.organization_id = $2",
    )
    .bind(product_id)
    .bind(org)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(ApiError::NotFound("Product"))
}

pub async fn create_product(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    req: &CreateProductRequest,
) -> Result<ProductRow> {
    let mut tx = pool.begin().await?;
    ensure_org_ref(&mut tx, "categories", org, req.category_id, "category_id").await?;
    if let Some(id) = req.manufacturer_id {
        ensure_org_ref(&mut tx, "manufacturers", org, id, "manufacturer_id").await?;
    }
    if let Some(id) = req.depreciation_model_id {
        ensure_org_ref(
            &mut tx,
            "depreciation_models",
            org,
            id,
            "depreciation_model_id",
        )
        .await?;
    }
    if let Some(id) = req.default_supplier_id {
        ensure_org_ref(&mut tx, "suppliers", org, id, "default_supplier_id").await?;
    }

    let code = match trimmed(&req.code) {
        Some(code) => normalize_code(&code),
        None => {
            let base = derive_code(&name_en(&req.translations));
            next_available_code(&mut tx, "products", org, &base).await?
        }
    };

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO products
            (organization_id, code, category_id, manufacturer_id, depreciation_model_id,
             model_code, eol_type, eol_date, default_purchase_cost, default_currency,
             default_supplier_id, min_quantity, chip, operating_system, notes,
             custom_fields_json, specs_json, is_archived, image_url)
         VALUES ($1, $2, $3, $4, $5, $6, COALESCE($7, 'individual'), $8, $9,
                 COALESCE($10, 'USD'), $11, COALESCE($12, 0), $13, $14, $15,
                 COALESCE($16, '{}'::jsonb), COALESCE($17, '{}'::jsonb), COALESCE($18, FALSE), $19)
         RETURNING id",
    )
    .bind(org)
    .bind(&code)
    .bind(req.category_id)
    .bind(req.manufacturer_id)
    .bind(req.depreciation_model_id)
    .bind(trimmed(&req.model_code))
    .bind(req.eol_type.as_deref())
    .bind(req.eol_date)
    .bind(req.default_purchase_cost)
    .bind(trimmed(&req.default_currency).map(|c| c.to_uppercase()))
    .bind(req.default_supplier_id)
    .bind(req.min_quantity)
    .bind(trimmed(&req.chip))
    .bind(trimmed(&req.operating_system))
    .bind(trimmed(&req.notes))
    .bind(req.custom_fields_json.as_ref())
    .bind(req.specs_json.as_ref())
    .bind(req.is_archived)
    .bind(trimmed(&req.image_url))
    .fetch_one(&mut *tx)
    .await?;

    for (locale_key, name) in &req.translations {
        sqlx::query(
            "INSERT INTO product_translations (product_id, locale, name) VALUES ($1, $2, $3)
             ON CONFLICT (product_id, locale) DO UPDATE SET name = EXCLUDED.name",
        )
        .bind(id)
        .bind(locale_key)
        .bind(name.trim())
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    get_product(pool, org, locale, id).await
}

pub async fn update_product(
    pool: &PgPool,
    org: Uuid,
    locale: &str,
    id: Uuid,
    req: &UpdateProductRequest,
) -> Result<ProductRow> {
    let current = get_product(pool, org, locale, id).await?;
    let mut tx = pool.begin().await?;

    if let Some(category_id) = req.category_id {
        ensure_org_ref(&mut tx, "categories", org, category_id, "category_id").await?;
    }
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
    if let Some(Some(depreciation_model_id)) = req.depreciation_model_id {
        ensure_org_ref(
            &mut tx,
            "depreciation_models",
            org,
            depreciation_model_id,
            "depreciation_model_id",
        )
        .await?;
    }
    if let Some(Some(supplier_id)) = req.default_supplier_id {
        ensure_org_ref(
            &mut tx,
            "suppliers",
            org,
            supplier_id,
            "default_supplier_id",
        )
        .await?;
    }

    if let Some(translations) = &req.translations {
        sqlx::query("DELETE FROM product_translations WHERE product_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        for (locale_key, name) in translations {
            sqlx::query(
                "INSERT INTO product_translations (product_id, locale, name) VALUES ($1, $2, $3)",
            )
            .bind(id)
            .bind(locale_key)
            .bind(name.trim())
            .execute(&mut *tx)
            .await?;
        }
    }

    let eol_type = req.eol_type.clone().unwrap_or(current.eol_type);
    let eol_date = req.eol_date.unwrap_or(current.eol_date);
    if eol_type == "fixed_date" && eol_date.is_none() {
        return Err(ApiError::bad_request(
            "eol_date",
            "required",
            "eol_date is required when eol_type is fixed_date",
        ));
    }

    sqlx::query(
        "UPDATE products
         SET code = $3, category_id = $4, manufacturer_id = $5, depreciation_model_id = $6,
             model_code = $7, eol_type = $8, eol_date = $9, default_purchase_cost = $10,
             default_currency = $11, default_supplier_id = $12, min_quantity = $13, chip = $14,
             operating_system = $15, notes = $16, custom_fields_json = $17, specs_json = $18,
             is_archived = $19, image_url = $20, updated_at = now()
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
    .bind(req.category_id.unwrap_or(current.category_id))
    .bind(req.manufacturer_id.unwrap_or(current.manufacturer_id))
    .bind(
        req.depreciation_model_id
            .unwrap_or(current.depreciation_model_id),
    )
    .bind(req.model_code.clone().unwrap_or(current.model_code))
    .bind(&eol_type)
    .bind(eol_date)
    .bind(
        req.default_purchase_cost
            .unwrap_or(current.default_purchase_cost),
    )
    .bind(
        req.default_currency
            .as_deref()
            .map(|c| c.trim().to_uppercase())
            .unwrap_or(current.default_currency),
    )
    .bind(
        req.default_supplier_id
            .unwrap_or(current.default_supplier_id),
    )
    .bind(req.min_quantity.unwrap_or(current.min_quantity))
    .bind(req.chip.clone().unwrap_or(current.chip))
    .bind(
        req.operating_system
            .clone()
            .unwrap_or(current.operating_system),
    )
    .bind(req.notes.clone().unwrap_or(current.notes))
    .bind(
        req.custom_fields_json
            .as_ref()
            .unwrap_or(&current.custom_fields_json),
    )
    .bind(req.specs_json.as_ref().unwrap_or(&current.specs_json))
    .bind(req.is_archived.unwrap_or(current.is_archived))
    .bind(req.image_url.clone().unwrap_or(current.image_url))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    get_product(pool, org, locale, id).await
}

pub async fn delete_product(pool: &PgPool, org: Uuid, id: Uuid) -> Result<()> {
    let done = sqlx::query("DELETE FROM products WHERE id = $1 AND organization_id = $2")
        .bind(id)
        .bind(org)
        .execute(pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound("Product"));
    }
    Ok(())
}
