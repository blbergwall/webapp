use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// Error type for framework handlers and the `Auth` extractor. Apps can use it
/// for their own handlers too (`?` converts any `anyhow::Error`).
#[derive(Debug)]
pub enum ApiError {
    /// No valid session.
    Unauthorized,
    /// Request refused, e.g. a cookie-authenticated write without a JSON
    /// content type (CSRF protection).
    Forbidden,
    /// Anything unexpected. Logged server-side; clients get a generic message.
    Internal(anyhow::Error),
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        Self::Internal(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized").into_response(),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden").into_response(),
            Self::Internal(err) => {
                eprintln!("internal error: {err:#}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
            }
        }
    }
}
