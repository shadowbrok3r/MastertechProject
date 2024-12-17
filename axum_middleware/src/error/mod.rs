use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt;
use uuid::Uuid;
pub mod context;
use context::Ctx;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ApiError {
    pub error: Error,
    pub req_id: Uuid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Error {
    Generic { description: String },
    Serde { source: String },
}

/// ApiError has to have the req_id to report to the client and implements IntoResponse.
pub type ApiResult<T> = core::result::Result<T, ApiError>;
/// Any error for storing before composing a response.
/// For errors that either don't affect the response, or are build before attaching the req_id.
pub type Result<T> = core::result::Result<T, Error>;

impl std::error::Error for Error {}
// We don't implement Error for ApiError, because it doesn't implement Display.

// for slightly less verbose error mappings
pub trait IntoApiError {
    fn into_api_error(self, ctx: &Ctx) -> ApiError;
}
impl<E: Into<Error>> IntoApiError for E {
    fn into_api_error(self, ctx: &Ctx) -> ApiError {
        ApiError {
            req_id: ctx.req_id(),
            error: self.into(),
        }
    }
}
impl ApiError {
    pub fn from<T: Into<Error>>(ctx: &Ctx) -> impl FnOnce(T) 
    -> ApiError + '_ {
        |e| e.into_api_error(ctx)
    }
}

const _INTERNAL: &str = "Internal error";
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Generic { description } => write!(f, "{description}"),
            Self::Serde { source } => write!(f, "Serde error - {source}"),
        }
    }
}

// REST error response
impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        println!("->> {:<12} - into_response - {self:?}", "ERROR");
        let status_code = match self.error {
            Error::Serde { .. } => StatusCode::BAD_REQUEST,
            Error::Generic { .. } => StatusCode::BAD_REQUEST,
        };
        let body = Json(json!({
            "error": {
                "error": self.error.to_string(),
                "req_id": self.req_id.to_string()
            }
        }));
        let mut response = (status_code, body).into_response();
        // Insert the real Error into the response - for the logger
        response.extensions_mut().insert(self.error);
        response
    }
}

// for sending serialized keys through gql extensions
pub const ERROR_SER_KEY: &str = "error_ser";


// External Errors
impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde {
            source: value.to_string(),
        }
    }
}
