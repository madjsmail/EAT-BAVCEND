// Organization settings: typed JSONB settings, locale and logo metadata.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::api_error::ApiError;
use crate::response::api_response::ErrorDetail;

/// Locales the product ships translations for; mirrors `users_preferred_locale_check`.
pub const SUPPORTED_LOCALES: [&str; 3] = ["en", "fr", "ar"];

pub const LOGO_CONTENT_TYPES: [&str; 3] = ["image/png", "image/jpeg", "image/webp"];
pub const MAX_LOGO_BYTES: u64 = 2 * 1024 * 1024;

pub fn is_supported_locale(value: &str) -> bool {
    SUPPORTED_LOCALES.contains(&value)
}

/// ISO 4217 shape check: three uppercase ASCII letters (`USD`, `EUR`, `XOF`).
pub fn is_currency_code(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 3 && bytes.iter().all(u8::is_ascii_uppercase)
}

fn field_error(field: &str, code: &str, message: &str) -> ErrorDetail {
    ErrorDetail {
        field: Some(field.to_string()),
        code: code.to_string(),
        message: message.to_string(),
    }
}

/// Typed view of `organizations.settings` (JSONB). Missing keys fall back to
/// [`Default`], so rows created before this milestone keep working.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OrgSettings {
    pub asset_tag_prefix: String,
    pub asset_tag_padding: u32,
    pub warranty_alert_days: i32,
    pub license_alert_days: i32,
    pub currency: String,
    pub require_handover_signature: bool,
}

impl Default for OrgSettings {
    fn default() -> Self {
        Self {
            asset_tag_prefix: "EAT-".to_string(),
            asset_tag_padding: 4,
            warranty_alert_days: 30,
            license_alert_days: 30,
            currency: "USD".to_string(),
            require_handover_signature: false,
        }
    }
}

impl OrgSettings {
    /// Validates values read back from storage (defensive; writes are validated
    /// by [`OrgSettingsUpdate::into_patch`]).
    pub fn validate(&self) -> Result<(), Vec<ErrorDetail>> {
        let mut errors = Vec::new();
        validate_prefix(&self.asset_tag_prefix, &mut errors);
        validate_padding(self.asset_tag_padding, &mut errors);
        validate_alert_days("warranty_alert_days", self.warranty_alert_days, &mut errors);
        validate_alert_days("license_alert_days", self.license_alert_days, &mut errors);
        validate_currency(&self.currency, &mut errors);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Partial settings update. Omitted fields keep their stored value; unknown
/// keys are rejected so typos do not silently disappear.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrgSettingsUpdate {
    pub asset_tag_prefix: Option<String>,
    pub asset_tag_padding: Option<u32>,
    pub warranty_alert_days: Option<i32>,
    pub license_alert_days: Option<i32>,
    pub currency: Option<String>,
    pub require_handover_signature: Option<bool>,
}

impl OrgSettingsUpdate {
    /// Normalizes values (trims, uppercases the currency) and returns the JSONB
    /// patch document containing only the supplied keys.
    pub fn into_patch(self) -> Result<serde_json::Value, ApiError> {
        let mut errors = Vec::new();
        let mut patch = serde_json::Map::new();

        if let Some(raw) = self.asset_tag_prefix {
            let prefix = raw.trim().to_string();
            let before = errors.len();
            validate_prefix(&prefix, &mut errors);
            if errors.len() == before {
                patch.insert("asset_tag_prefix".into(), prefix.into());
            }
        }
        if let Some(padding) = self.asset_tag_padding {
            let before = errors.len();
            validate_padding(padding, &mut errors);
            if errors.len() == before {
                patch.insert("asset_tag_padding".into(), padding.into());
            }
        }
        if let Some(days) = self.warranty_alert_days {
            let before = errors.len();
            validate_alert_days("warranty_alert_days", days, &mut errors);
            if errors.len() == before {
                patch.insert("warranty_alert_days".into(), days.into());
            }
        }
        if let Some(days) = self.license_alert_days {
            let before = errors.len();
            validate_alert_days("license_alert_days", days, &mut errors);
            if errors.len() == before {
                patch.insert("license_alert_days".into(), days.into());
            }
        }
        if let Some(raw) = self.currency {
            let currency = raw.trim().to_uppercase();
            let before = errors.len();
            validate_currency(&currency, &mut errors);
            if errors.len() == before {
                patch.insert("currency".into(), currency.into());
            }
        }
        if let Some(required) = self.require_handover_signature {
            patch.insert("require_handover_signature".into(), required.into());
        }

        if errors.is_empty() {
            Ok(serde_json::Value::Object(patch))
        } else {
            Err(ApiError::Validation(errors))
        }
    }
}

fn validate_prefix(prefix: &str, errors: &mut Vec<ErrorDetail>) {
    if prefix.trim().is_empty() {
        errors.push(field_error(
            "asset_tag_prefix",
            "required",
            "asset_tag_prefix cannot be empty",
        ));
    } else if prefix.chars().count() > 10 {
        errors.push(field_error(
            "asset_tag_prefix",
            "too_long",
            "asset_tag_prefix must be at most 10 characters",
        ));
    } else if prefix.chars().any(char::is_whitespace) {
        errors.push(field_error(
            "asset_tag_prefix",
            "invalid",
            "asset_tag_prefix cannot contain whitespace",
        ));
    }
}

fn validate_padding(padding: u32, errors: &mut Vec<ErrorDetail>) {
    if !(1..=10).contains(&padding) {
        errors.push(field_error(
            "asset_tag_padding",
            "out_of_range",
            "asset_tag_padding must be between 1 and 10",
        ));
    }
}

fn validate_alert_days(field: &str, days: i32, errors: &mut Vec<ErrorDetail>) {
    if !(0..=3650).contains(&days) {
        errors.push(field_error(
            field,
            "out_of_range",
            "Alert days must be between 0 and 3650",
        ));
    }
}

fn validate_currency(currency: &str, errors: &mut Vec<ErrorDetail>) {
    if !is_currency_code(currency) {
        errors.push(field_error(
            "currency",
            "invalid",
            "currency must be a 3-letter ISO 4217 code",
        ));
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateOrganizationRequest {
    pub name: Option<String>,
    pub default_locale: Option<String>,
    /// Garage object key returned by `POST /organization/logo/upload-url`.
    pub logo_object_key: Option<String>,
    pub settings: Option<OrgSettingsUpdate>,
}

#[derive(Debug, Serialize)]
pub struct OrganizationResponse {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub default_locale: String,
    pub logo_object_key: Option<String>,
    /// Presigned GET, valid for 15 minutes. `None` when no logo is set.
    pub logo_url: Option<String>,
    pub settings: OrgSettings,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct LogoUploadUrlRequest {
    pub file_name: String,
    pub content_type: String,
}

#[derive(Debug, Serialize)]
pub struct LogoUploadUrlResponse {
    pub object_key: String,
    pub upload_url: String,
    pub expires_in_seconds: u64,
    pub max_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(json: serde_json::Value) -> Result<serde_json::Value, ApiError> {
        serde_json::from_value::<OrgSettingsUpdate>(json)
            .map_err(|e| ApiError::invalid(e.to_string()))?
            .into_patch()
    }

    #[test]
    fn defaults_are_valid() {
        assert!(OrgSettings::default().validate().is_ok());
    }

    #[test]
    fn partial_update_only_contains_given_keys() {
        let out = patch(serde_json::json!({ "currency": " eur " })).unwrap();
        assert_eq!(out, serde_json::json!({ "currency": "EUR" }));
    }

    #[test]
    fn padding_bounds_are_enforced() {
        assert!(patch(serde_json::json!({ "asset_tag_padding": 1 })).is_ok());
        assert!(patch(serde_json::json!({ "asset_tag_padding": 10 })).is_ok());
        assert!(patch(serde_json::json!({ "asset_tag_padding": 0 })).is_err());
        assert!(patch(serde_json::json!({ "asset_tag_padding": 11 })).is_err());
    }

    #[test]
    fn alert_days_must_be_non_negative() {
        assert!(patch(serde_json::json!({ "warranty_alert_days": 0 })).is_ok());
        assert!(patch(serde_json::json!({ "license_alert_days": -1 })).is_err());
    }

    #[test]
    fn currency_must_be_three_letters() {
        assert!(patch(serde_json::json!({ "currency": "usd" })).is_ok());
        assert!(patch(serde_json::json!({ "currency": "US" })).is_err());
        assert!(patch(serde_json::json!({ "currency": "US1" })).is_err());
    }

    #[test]
    fn prefix_must_not_be_blank_or_long() {
        assert!(patch(serde_json::json!({ "asset_tag_prefix": "   " })).is_err());
        assert!(patch(serde_json::json!({ "asset_tag_prefix": "ABCDEFGHIJK" })).is_err());
        assert!(patch(serde_json::json!({ "asset_tag_prefix": "EAT" })).is_ok());
    }

    #[test]
    fn unknown_settings_key_is_rejected() {
        assert!(
            serde_json::from_value::<OrgSettingsUpdate>(
                serde_json::json!({ "asset_tag_paddin": 4 })
            )
            .is_err()
        );
    }
}
