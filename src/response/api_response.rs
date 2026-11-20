// Envelope helpers are the project-wide response API; not every variant is used yet.
#![allow(dead_code)]

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// Standard envelope returned by every endpoint.
#[derive(Clone, Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub error: bool,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pagination: Option<Pagination>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_stack: Option<Vec<ErrorDetail>>,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Pagination {
    pub page: u32,
    pub per_page: u32,
    pub total: u64,
    pub total_pages: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct ErrorDetail {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub code: String,
    pub message: String,
}

impl<T: Serialize> ApiResponse<T> {
    /// Single-object success response.
    pub fn success(data: T) -> Self {
        Self {
            error: false,
            status: 200,
            data: Some(data),
            pagination: None,
            error_stack: None,
            message: "success".to_string(),
        }
    }

    /// Single-object success response with a custom message.
    pub fn success_with_message(data: T, message: impl Into<String>) -> Self {
        Self {
            error: false,
            status: 200,
            data: Some(data),
            pagination: None,
            error_stack: None,
            message: message.into(),
        }
    }

    /// Success response for a list, including pagination metadata.
    pub fn paginated(data: Vec<T>, page: u32, per_page: u32, total: u64) -> ApiResponse<Vec<T>> {
        ApiResponse {
            error: false,
            status: 200,
            data: Some(data),
            pagination: Some(Pagination::new(page, per_page, total)),
            error_stack: None,
            message: "success".to_string(),
        }
    }

    /// Error response. `error_stack` carries the per-field/per-cause details.
    pub fn error(status: u16, message: impl Into<String>, error_stack: Vec<ErrorDetail>) -> Self {
        Self {
            error: true,
            status,
            data: None,
            pagination: None,
            error_stack: Some(error_stack),
            message: message.into(),
        }
    }
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(self)).into_response()
    }
}

impl Pagination {
    pub fn new(page: u32, per_page: u32, total: u64) -> Self {
        let per_page = per_page.max(1);
        let total_pages = total.div_ceil(per_page as u64) as u32;
        Self {
            page,
            per_page,
            total,
            total_pages,
        }
    }
}

impl ErrorDetail {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: None,
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn with_field(
        field: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            field: Some(field.into()),
            code: code.into(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_response_shape() {
        let value = serde_json::to_value(ApiResponse::success(42)).unwrap();
        assert_eq!(value["error"], false);
        assert_eq!(value["status"], 200);
        assert_eq!(value["data"], 42);
        assert_eq!(value["message"], "success");
        assert!(value.get("pagination").is_none());
        assert!(value.get("error_stack").is_none());
    }

    #[test]
    fn paginated_response_shape() {
        let value = serde_json::to_value(ApiResponse::paginated(vec![1, 2, 3], 2, 10, 25)).unwrap();
        assert_eq!(value["error"], false);
        assert_eq!(value["data"], serde_json::json!([1, 2, 3]));
        assert_eq!(value["pagination"]["page"], 2);
        assert_eq!(value["pagination"]["per_page"], 10);
        assert_eq!(value["pagination"]["total"], 25);
        assert_eq!(value["pagination"]["total_pages"], 3);
    }

    #[test]
    fn error_response_shape() {
        let value = serde_json::to_value(ApiResponse::<()>::error(
            400,
            "Validation failed",
            vec![ErrorDetail::with_field(
                "email",
                "invalid",
                "Email is not valid",
            )],
        ))
        .unwrap();
        assert_eq!(value["error"], true);
        assert_eq!(value["status"], 400);
        assert!(value.get("data").is_none());
        assert!(value.get("pagination").is_none());
        assert_eq!(value["error_stack"][0]["field"], "email");
        assert_eq!(value["error_stack"][0]["code"], "invalid");
        assert_eq!(value["error_stack"][0]["message"], "Email is not valid");
    }
}
