//! HTTP error type with OAuth-style JSON bodies.

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug)]
pub struct AppError {
    status: StatusCode,
    error: String,
    description: String,
}

impl AppError {
    pub fn new(
        status: StatusCode,
        error: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            status,
            error: error.into(),
            description: description.into(),
        }
    }

    pub fn invalid_request(description: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_request", description)
    }

    pub fn invalid_client(description: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "invalid_client", description)
    }

    pub fn invalid_grant(description: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_grant", description)
    }

    pub fn access_denied(description: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "access_denied", description)
    }

    pub fn unauthorized(description: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "invalid_token", description)
    }

    pub fn not_found(description: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", description)
    }

    pub fn unsupported_grant_type(description: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "unsupported_grant_type",
            description,
        )
    }

    pub fn internal(description: impl Into<String>) -> Self {
        let description = description.into();
        tracing::error!(error = %description, "internal error");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            "internal server error",
        )
    }

    pub fn too_many_requests(description: impl Into<String>) -> Self {
        Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "temporarily_unavailable",
            description,
        )
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            self.status,
            [
                (header::CONTENT_TYPE, "application/json"),
                (header::CACHE_CONTROL, "no-store"),
                (header::PRAGMA, "no-cache"),
            ],
            Json(json!({
                "error": self.error,
                "error_description": self.description,
            })),
        )
            .into_response()
    }
}
