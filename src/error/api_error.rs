#![allow(dead_code)]

use crate::response::api_response::{ApiResponse, ErrorDetail};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Authentication required")]
    Unauthorized,
    #[error("Missing permission: {0}")]
    Forbidden(String),
    #[error("{0} not found")]
    NotFound(&'static str),
    #[error("{message}")]
    BadRequest {
        field: Option<String>,
        code: String,
        message: String,
    },
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Internal(String),
    #[error(transparent)]
    Db(sqlx::Error),
}

impl ApiError {
    pub fn bad_request(
        field: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::BadRequest {
            field: Some(field.into()),
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::BadRequest {
            field: None,
            code: "invalid".to_string(),
            message: message.into(),
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        match &e {
            sqlx::Error::Database(db) if db.is_unique_violation() => {
                ApiError::Conflict("Resource already exists".to_string())
            }
            _ => ApiError::Db(e),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let ApiError::Db(ref e) = self {
            log::error!("Database error: {e}");
        }

        let (status, code, message, field) = match self {
            ApiError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Authentication required".to_string(),
                None,
            ),
            ApiError::Forbidden(perm) => (
                StatusCode::FORBIDDEN,
                "forbidden",
                format!("Missing permission: {perm}"),
                None,
            ),
            ApiError::NotFound(what) => (
                StatusCode::NOT_FOUND,
                "not_found",
                format!("{what} not found"),
                None,
            ),
            ApiError::BadRequest {
                field,
                code: _,
                message,
            } => (StatusCode::BAD_REQUEST, "invalid", message, field),
            ApiError::Conflict(message) => (StatusCode::CONFLICT, "conflict", message, None),
            ApiError::Internal(message) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                message,
                None,
            ),
            ApiError::Db(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Database error".to_string(),
                None,
            ),
        };

        let detail = ErrorDetail {
            field,
            code: code.to_string(),
            message: message.clone(),
        };

        ApiResponse::<()>::error(status.as_u16(), message, vec![detail]).into_response()
    }
}
