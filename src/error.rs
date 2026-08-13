use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("not found")]
    NotFound,

    #[error("{0}")]
    Invalid(String),
}

impl AppError {
    pub fn status(&self) -> StatusCode {
        match self {
            AppError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::Invalid(_) => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();

        // Database internals are logged, never shown to the caller.
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %self, "request failed");
            return (
                status,
                axum::Json(serde_json::json!({ "error": "internal server error" })),
            )
                .into_response();
        }

        (
            status,
            axum::Json(serde_json::json!({ "error": self.to_string() })),
        )
            .into_response()
    }
}
