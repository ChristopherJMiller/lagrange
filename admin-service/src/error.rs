use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("ip pool exhausted")]
    IpPoolExhausted,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("subprocess failed: {0}")]
    Subprocess(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
    detail: Option<String>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, detail) = match &self {
            ApiError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found", Some(self.to_string())),
            ApiError::Conflict(_) => (StatusCode::CONFLICT, "conflict", Some(self.to_string())),
            ApiError::BadRequest(_) => (
                StatusCode::BAD_REQUEST,
                "bad_request",
                Some(self.to_string()),
            ),
            ApiError::IpPoolExhausted => (
                StatusCode::SERVICE_UNAVAILABLE,
                "ip_pool_exhausted",
                None,
            ),
            ApiError::Database(_) | ApiError::Io(_) | ApiError::Subprocess(_) | ApiError::Other(_) => {
                tracing::error!(error = ?self, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    Some(self.to_string()),
                )
            }
        };

        let body = serde_json::to_vec(&ErrorBody {
            error: code.to_string(),
            detail,
        })
        .unwrap_or_default();

        (
            status,
            [("content-type", "application/json")],
            body,
        )
            .into_response()
    }
}

pub type ApiResult<T> = std::result::Result<T, ApiError>;
