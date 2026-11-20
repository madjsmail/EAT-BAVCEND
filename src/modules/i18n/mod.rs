// Locale resolution for localized lookup names.
//
// Lookups store one row per locale in `<entity>_translations`. Reads resolve
// `translations[locale] ?? translations["en"]`.

use std::collections::BTreeMap;

use axum::extract::FromRequestParts;
use axum::http::header::ACCEPT_LANGUAGE;
use axum::http::request::Parts;
use serde::{Deserialize, Serialize};

/// Locales the product ships with. Adding one is a migration (locale CHECK)
/// plus an entry here.
pub const SUPPORTED_LOCALES: [&str; 3] = ["en", "fr", "ar"];
pub const DEFAULT_LOCALE: &str = "en";

pub type Translations = BTreeMap<String, String>;

/// `Accept-Language` parsed down to a supported base tag (`fr-CA` -> `fr`).
#[derive(Debug, Clone, Default)]
pub struct Locale(pub Option<String>);

impl<S: Send + Sync> FromRequestParts<S> for Locale {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(
            parts
                .headers
                .get(ACCEPT_LANGUAGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_accept_language),
        ))
    }
}

impl Locale {
    /// Header locale, else the signed-in user's preference, else English.
    pub fn resolve(&self, user_preference: &str) -> String {
        self.0
            .clone()
            .or_else(|| supported(user_preference))
            .unwrap_or_else(|| DEFAULT_LOCALE.to_string())
    }
}

/// Highest-q supported language in an `Accept-Language` header.
pub fn parse_accept_language(raw: &str) -> Option<String> {
    let mut best: Option<(f32, String)> = None;
    for entry in raw.split(',') {
        let mut parts = entry.split(';');
        let tag = parts.next().unwrap_or("").trim();
        if tag.is_empty() {
            continue;
        }
        let quality = parts
            .find_map(|p| p.trim().strip_prefix("q="))
            .and_then(|q| q.parse::<f32>().ok())
            .unwrap_or(1.0);
        let Some(locale) = supported(tag) else {
            continue;
        };
        if best.as_ref().is_none_or(|(best_q, _)| quality > *best_q) {
            best = Some((quality, locale));
        }
    }
    best.map(|(_, locale)| locale)
}

/// `fr-CA` -> `fr` when supported, otherwise `None`.
fn supported(tag: &str) -> Option<String> {
    let base = tag.split(['-', '_']).next()?.to_ascii_lowercase();
    SUPPORTED_LOCALES.contains(&base.as_str()).then_some(base)
}

/// Resolve a localized name: requested locale, then English, then any locale.
pub fn pick(translations: &Translations, locale: &str) -> String {
    translations
        .get(locale)
        .or_else(|| translations.get(DEFAULT_LOCALE))
        .or_else(|| translations.values().next())
        .cloned()
        .unwrap_or_default()
}

/// Validate a translation map coming from a request body.
/// At least one supported locale with a non-blank name is required.
pub fn validate(translations: &Translations) -> Result<(), &'static str> {
    if translations.is_empty() {
        return Err("at least one locale name is required");
    }
    for (locale, name) in translations {
        if !SUPPORTED_LOCALES.contains(&locale.as_str()) {
            return Err("unsupported locale");
        }
        if name.trim().is_empty() {
            return Err("translated name cannot be blank");
        }
    }
    Ok(())
}

/// A translation map as stored/returned in API payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TranslationMap(pub Translations);

/// Serialized lookup name plus every locale, so admin screens can edit them.
pub fn resolved(translations: &Translations, locale: &str) -> (String, Translations) {
    (pick(translations, locale), translations.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_requested_then_english_then_any() {
        let map: Translations = [
            ("en".to_string(), "Deployed".to_string()),
            ("fr".to_string(), "Déployé".to_string()),
        ]
        .into_iter()
        .collect();

        assert_eq!(pick(&map, "fr"), "Déployé");
        assert_eq!(pick(&map, "ar"), "Deployed");
        assert_eq!(pick(&map, "en"), "Deployed");
    }

    #[test]
    fn parses_quality_ordering_and_region() {
        assert_eq!(
            parse_accept_language("fr-CA,fr;q=0.9,en;q=0.5").as_deref(),
            Some("fr")
        );
        assert_eq!(
            parse_accept_language("de-DE,ar;q=0.8,en;q=0.7").as_deref(),
            Some("ar")
        );
        assert_eq!(parse_accept_language("en-US"), Some("en".to_string()));
        assert_eq!(parse_accept_language("de-DE"), None);
    }

    #[test]
    fn resolves_header_then_user_preference() {
        assert_eq!(Locale(Some("ar".into())).resolve("fr"), "ar");
        assert_eq!(Locale(None).resolve("fr"), "fr");
        assert_eq!(Locale(None).resolve("de"), "en");
    }

    #[test]
    fn rejects_empty_and_unknown_locales() {
        assert!(validate(&Translations::new()).is_err());
        let mut bad = Translations::new();
        bad.insert("de".into(), "Deployed".into());
        assert!(validate(&bad).is_err());
        let mut ok = Translations::new();
        ok.insert("ar".into(), "موزع".into());
        assert!(validate(&ok).is_ok());
    }
}
