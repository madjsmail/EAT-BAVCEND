// Small serde/validation helpers shared by the milestone-2 modules.

use serde::{Deserialize, Deserializer};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::api_error::ApiError;

/// Basic deliverable email shape check; callers attach the field error.
pub fn valid_email(email: &str) -> bool {
    email.len() <= 254
        && email.contains('@')
        && !email.starts_with('@')
        && !email.ends_with('@')
        && !email.contains(' ')
}

/// Distinguishes three PATCH states for a nullable field:
/// absent -> `None`, explicit `null` -> `Some(None)`, value -> `Some(Some(v))`.
pub fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Ok(Some(Option::<T>::deserialize(deserializer)?))
}

/// PATCH convention: an empty string clears a nullable text column.
pub fn blank_to_null(value: Option<String>) -> Option<Option<String>> {
    value.map(|v| {
        let trimmed = v.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

/// Same convention for single-line codes: trim + uppercase.
pub fn normalize_code(raw: &str) -> String {
    raw.trim().to_uppercase()
}

/// Reject a value outside a fixed enum, as a 400 on the given field.
pub fn validate_enum(field: &str, value: &str, allowed: &[&str]) -> Result<(), ApiError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            field,
            "invalid",
            format!("{field} must be one of: {}", allowed.join(", ")),
        ))
    }
}

/// Reject a foreign key that points at another tenant's row.
/// `table` is an internal constant, never user input.
pub async fn ensure_org_ref(
    conn: &mut PgConnection,
    table: &str,
    organization_id: Uuid,
    id: Uuid,
    field: &str,
) -> Result<(), ApiError> {
    let found: bool = sqlx::query_scalar(&format!(
        "SELECT EXISTS(SELECT 1 FROM {table} WHERE id = $1 AND organization_id = $2)"
    ))
    .bind(id)
    .bind(organization_id)
    .fetch_one(&mut *conn)
    .await?;
    if !found {
        return Err(ApiError::bad_request(
            field,
            "not_found",
            format!("{field} does not exist in this organization"),
        ));
    }
    Ok(())
}

/// First free `code` for a lookup table, appending a numeric suffix on collision.
/// `table` is an internal constant, never user input.
/// ponytail: check-then-insert races with a concurrent create; the UNIQUE
/// constraint turns that into a 409 instead of a duplicate row.
pub async fn next_available_code(
    conn: &mut PgConnection,
    table: &str,
    organization_id: Uuid,
    base: &str,
) -> sqlx::Result<String> {
    for attempt in 0..100u32 {
        let candidate = if attempt == 0 {
            base.to_string()
        } else {
            let suffix = attempt.to_string();
            let keep = 8usize.saturating_sub(suffix.len());
            format!("{}{}", base.chars().take(keep).collect::<String>(), suffix)
        };
        let taken: bool = sqlx::query_scalar(&format!(
            "SELECT EXISTS(SELECT 1 FROM {table} WHERE organization_id = $1 AND code = $2)"
        ))
        .bind(organization_id)
        .bind(&candidate)
        .fetch_one(&mut *conn)
        .await?;
        if !taken {
            return Ok(candidate);
        }
    }
    Ok(base.to_string())
}

/// Derive a lowercase snake_case machine key (max 32 chars) from a name.
pub fn derive_slug(name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('_') {
            slug.push('_');
        }
        if slug.len() >= 32 {
            break;
        }
    }
    let slug = slug.trim_matches('_').to_string();
    if slug.is_empty() {
        "status".to_string()
    } else {
        slug
    }
}

/// Derive an uppercase A-Z0-9 code (max 8 chars) from a name.
/// Falls back to `"GEN"` when the name has no usable characters.
pub fn derive_code(name: &str) -> String {
    let mut code: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect::<String>()
        .to_uppercase();
    if code.is_empty() {
        code.push_str("GEN");
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_codes_from_names() {
        assert_eq!(derive_code("Communication"), "COMMUNIC");
        assert_eq!(derive_code("IT"), "IT");
        assert_eq!(derive_code("خدمة"), "GEN");
        assert_eq!(derive_code("Finance dept"), "FINANCED");
    }
}
