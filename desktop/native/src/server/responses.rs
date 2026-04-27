use super::*;
use axum::http::StatusCode;
use axum::response::IntoResponse;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ErrorResponse {
    code: String,
    pub(super) error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct OkResponse {
    pub(super) status: String,
}

pub(super) fn error_response(
    status: StatusCode,
    code: &'static str,
    message: impl Into<String>,
) -> (StatusCode, Json<ErrorResponse>) {
    (
        status,
        Json(ErrorResponse {
            code: code.to_string(),
            error: message.into(),
        }),
    )
}

pub(super) fn json_error_response(
    status: StatusCode,
    code: &'static str,
    message: impl Into<String>,
) -> axum::response::Response {
    error_response(status, code, message).into_response()
}

pub(super) fn json_error_response_with_headers<const N: usize>(
    status: StatusCode,
    headers: [(axum::http::HeaderName, &'static str); N],
    code: &'static str,
    message: impl Into<String>,
) -> axum::response::Response {
    let (_, body) = error_response(status, code, message);
    (status, headers, body).into_response()
}

pub(super) type HandlerError = (StatusCode, Json<ErrorResponse>);
pub(super) type HandlerResult<T> = Result<T, HandlerError>;
