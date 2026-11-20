// Error plumbing used by handlers as endpoints are added.
#![allow(dead_code)]

use crate::response::api_response::{ApiResponse, ErrorDetail};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("{0}")]
    SomethingWentWrong(String),
    #[error("Duplicate entry exists")]
    UniqueConstraintViolation(String),
}

impl IntoResponse for DbError {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            DbError::SomethingWentWrong(_) => (StatusCode::INTERNAL_SERVER_ERROR, "db_error"),
            DbError::UniqueConstraintViolation(_) => (StatusCode::CONFLICT, "unique_constraint"),
        };

        let detail = ErrorDetail::new(code, self.to_string());
        ApiResponse::<()>::error(status.as_u16(), "Database error", vec![detail]).into_response()
    }
}
