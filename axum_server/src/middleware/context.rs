use crate::error::*;
use axum::extract::FromRequestParts;
use http::request::Parts;
use log::debug;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct Ctx {
    result_user_id: Result<String>,
    req_id: Uuid,
}

impl Ctx {
    pub fn new(result_user_id: Result<String>, uuid: Uuid) -> Self {
        Self {
            result_user_id,
            req_id: uuid,
        }
    }

    pub fn user_id(&self) -> ApiResult<String> {
        self.result_user_id.clone().map_err(|error| ApiError {
            error,
            req_id: self.req_id,
        })
    }

    pub fn req_id(&self) -> Uuid {
        self.req_id
    }
}

// ugly but direct implementation from axum, until "async trait fn" are in stable rust, instead of importing some 3rd party macro
// Extractor - makes it possible to specify Ctx as a param - fetches the result from the header parts extension
impl<S: Send + Sync> FromRequestParts<S> for Ctx {
    type Rejection = ApiError;
    
    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    )
        -> impl Future<Output = ApiResult<Self>> + Send
    {
        Box::pin(async {
            debug!(
                "->> {:<12} - Ctx::from_request_parts - extract Ctx from extension",
                "EXTRACTOR"
            );
            // A path that bypassed the recorder still gets its own req_id.
            Ok(parts.extensions.get::<Ctx>().cloned().unwrap_or_else(|| {
                Ctx::new(Ok("Shadowbroker".to_string()), Uuid::new_v4())
            }))
        })
    }
}