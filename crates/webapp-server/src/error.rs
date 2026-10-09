use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// Error type for framework handlers and the `Auth` extractor. Apps can use it
/// for their own handlers too (`?` converts any `anyhow::Error`).
#[derive(Debug)]
pub enum ApiError {
    /// Malformed input (400).
    BadRequest,
    /// No valid session (401).
    Unauthorized,
    /// Request refused, e.g. a cookie-authenticated write without a JSON
    /// content type (CSRF protection) (403).
    Forbidden,
    /// Feature not enabled on this server (404).
    NotFound,
    /// Request can't be completed automatically; the client should ask the
    /// user to confirm (409).
    Conflict,
    /// The thing referred to existed but is expired or used up (410).
    Gone,
    /// Rate limited (429).
    TooManyRequests,
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
        let (status, message) = match self {
            Self::BadRequest => (StatusCode::BAD_REQUEST, "bad request"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not found"),
            Self::Conflict => (StatusCode::CONFLICT, "confirmation required"),
            Self::Gone => (StatusCode::GONE, "expired or already used"),
            Self::TooManyRequests => (StatusCode::TOO_MANY_REQUESTS, "too many requests"),
            Self::Internal(err) => {
                eprintln!("internal error: {err:#}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error")
            }
        };
        (status, message).into_response()
    }
}
