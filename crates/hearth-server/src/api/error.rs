use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::services;
use crate::tools::ToolError;

/// Every handler returns this; it maps domain errors to status codes.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl ApiError {
    pub fn bad_request(m: impl Into<String>) -> Self {
        ApiError {
            status: StatusCode::BAD_REQUEST,
            message: m.into(),
        }
    }
    pub fn forbidden(m: impl Into<String>) -> Self {
        ApiError {
            status: StatusCode::FORBIDDEN,
            message: m.into(),
        }
    }
    pub fn not_found() -> Self {
        ApiError {
            status: StatusCode::NOT_FOUND,
            message: "not found".into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        // Only a true 500 hides its message; a 502 from an upstream (Claude, Plaid) is the caller's to read.
        if self.status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %self.message, "request failed");
            return (self.status, Json(json!({ "error": "internal error" }))).into_response();
        }
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

impl From<services::Error> for ApiError {
    fn from(e: services::Error) -> Self {
        match e {
            services::Error::NotFound => ApiError::not_found(),
            services::Error::Invalid(m) => ApiError::bad_request(m),
            services::Error::Conflict(m) => ApiError {
                status: StatusCode::CONFLICT,
                message: m,
            },
            services::Error::Forbidden => ApiError::forbidden("forbidden"),
            services::Error::Db(e) => ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: e.to_string(),
            },
            services::Error::Other(e) => ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: e.to_string(),
            },
        }
    }
}

impl From<ToolError> for ApiError {
    fn from(e: ToolError) -> Self {
        match e {
            ToolError::Unknown(n) => ApiError {
                status: StatusCode::NOT_FOUND,
                message: format!("unknown tool {n}"),
            },
            ToolError::Args(m) => ApiError::bad_request(m),
            ToolError::ReadOnly => ApiError::forbidden("this key is read-only"),
            ToolError::Service(s) => s.into(),
            ToolError::Json(e) => ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: e.to_string(),
            },
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        }
    }
}
