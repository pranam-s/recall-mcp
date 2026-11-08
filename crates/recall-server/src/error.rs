//! Mapping of core errors onto HTTP responses plus the API-wide JSON error
//! contract (framework rejections included).

use axum::Json;
use axum::extract::FromRequest;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use recall_core::RecallError;

/// A core error paired with its HTTP mapping.
#[derive(Debug)]
pub struct HttpError(pub RecallError);

impl From<RecallError> for HttpError {
    fn from(e: RecallError) -> Self {
        HttpError(e)
    }
}

impl From<serde_json::Error> for HttpError {
    fn from(e: serde_json::Error) -> Self {
        HttpError(RecallError::Serialization(e))
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let (status, message) = match &self.0 {
            RecallError::NamespaceNotFound(_) => (StatusCode::NOT_FOUND, self.0.to_string()),
            RecallError::MemoryNotFound(_) => (StatusCode::NOT_FOUND, self.0.to_string()),
            RecallError::DuplicateNamespace(_) => (StatusCode::CONFLICT, self.0.to_string()),
            RecallError::InvalidInput(_) => (StatusCode::BAD_REQUEST, self.0.to_string()),
            // Everything else is a server fault; details go to logs, not clients.
            other => {
                tracing::error!(error = %other, "internal error handling request");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
        };
        json_error(status, &message)
    }
}

/// One JSON error shape for every non-2xx API response.
pub fn json_error(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}

/// Rejection used by the auth middleware (structured like HttpError bodies).
pub fn unauthorized(detail: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [("WWW-Authenticate", "Bearer")],
        Json(serde_json::json!({ "error": detail })),
    )
        .into_response()
}

/// Rejection used by the rate-limit middleware, with the standard
/// `Retry-After` hint in whole seconds.
pub fn too_many_requests(retry_after_secs: u64) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [("Retry-After", retry_after_secs.max(1).to_string())],
        Json(serde_json::json!({ "error": "rate limit exceeded" })),
    )
        .into_response()
}

/// Axum `Json` extractor whose rejections are JSON too. Malformed JSON,
/// missing/wrong content type, and body-limit failures all come back as
/// `{"error": ...}` with 400/415/413 instead of axum's plain-text defaults.
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned,
{
    type Rejection = Response;

    async fn from_request(
        req: axum::http::Request<axum::body::Body>,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(value)) => Ok(ApiJson(value)),
            Err(rejection) => {
                let status = rejection.status();
                let message = match status {
                    StatusCode::PAYLOAD_TOO_LARGE => "payload too large".to_string(),
                    StatusCode::UNSUPPORTED_MEDIA_TYPE => {
                        "unsupported media type: send application/json".to_string()
                    }
                    _ => format!("invalid JSON request body: {}", rejection.body_text()),
                };
                Err(json_error(status, &message))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn maps_errors_to_statuses() {
        let cases = [
            (
                RecallError::NamespaceNotFound("w".into()),
                StatusCode::NOT_FOUND,
            ),
            (
                RecallError::MemoryNotFound("m".into()),
                StatusCode::NOT_FOUND,
            ),
            (
                RecallError::DuplicateNamespace("w".into()),
                StatusCode::CONFLICT,
            ),
            (
                RecallError::InvalidInput("x".into()),
                StatusCode::BAD_REQUEST,
            ),
            (
                RecallError::Io(std::io::Error::other("db down")),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];
        for (err, expected) in cases {
            let res = HttpError(err).into_response();
            assert_eq!(res.status(), expected);
        }
        let res = unauthorized("nope").into_response();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
}
