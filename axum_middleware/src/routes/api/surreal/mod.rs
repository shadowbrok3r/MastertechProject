use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{context::Ctx, ApiError};

#[derive(Default, Serialize, Deserialize)]
pub struct TestRes {
    value: Value
}

pub async fn handle_response(
    // _ctx: Ctx,
    Json(payload): Json<Value>
) -> Json<Result<TestRes, ApiError>> { 
    println!("Payload: {payload:?}");

    Json(Ok(TestRes::default()))
}