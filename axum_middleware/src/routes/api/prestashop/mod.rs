use crate::{error::ApiError, middleware::context::Ctx};
use tokio_cron_scheduler::{Job, JobScheduler};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use axum::Json;



#[derive(Default, Serialize, Deserialize)]
pub struct TestRes {
    value: Value
}

pub async fn handle_prestashop_response(
    ctx: Ctx,
    Json(payload): Json<Value>
) -> Result<Json<TestRes>, ApiError> { 
    println!("Payload: {payload:?}");
    start_cron_job(ctx).await;
    Ok(Json(TestRes::default()))
}


async fn start_cron_job(ctx: Ctx) {
    let _ = tokio::spawn(async move {
        let sched = JobScheduler::new()
        .await
        .map_err(|e| ApiError {
            error: crate::error::Error::Generic { description: e.to_string() },
            req_id: ctx.req_id(),
        })?;
    
        // Add basic cron job
        sched.add(
            Job::new("1/10 * * * * *", |_uuid, _l| {
                println!("I run every 10 seconds");
            }).map_err(|e| ApiError {
                error: crate::error::Error::Generic { description: e.to_string() },
                req_id: ctx.req_id(),
            })?
        ).await.map_err(|e| ApiError {
            error: crate::error::Error::Generic { description: e.to_string() },
            req_id: ctx.req_id(),
        })?;
    
        // Add async job
        sched.add(
            Job::new_async("1/7 * * * * *", |uuid, mut l| {
                Box::pin(async move {
                    println!("I run async every 7 seconds");
    
                    // Query the next execution time for this job
                    let next_tick = l.next_tick_for_job(uuid).await;
                    match next_tick {
                        Ok(Some(ts)) => println!("Next time for 7s job is {:?}", ts),
                        _ => println!("Could not get next tick for 7s job"),
                    }
                })
            }).map_err(|e| ApiError {
                error: crate::error::Error::Generic { description: e.to_string() },
                req_id: ctx.req_id(),
            })?
        ).await.map_err(|e| ApiError {
                error: crate::error::Error::Generic { description: e.to_string() },
                req_id: ctx.req_id(),
            })?;
    
        // Needs the `english` feature enabled
        let _ = sched.add(
            Job::new_async("every 4 seconds", |uuid, mut l| {
                Box::pin(async move {
                    println!("I run async every 4 seconds");
    
                    // Query the next execution time for this job
                    let next_tick = l.next_tick_for_job(uuid).await;
                    match next_tick {
                        Ok(Some(ts)) => println!("Next time for 4s job is {:?}", ts),
                        _ => println!("Could not get next tick for 4s job"),
                    }
                })
            }).map_err(|e| ApiError {
                error: crate::error::Error::Generic { description: e.to_string() },
                req_id: ctx.req_id(),
            })?
        ).await;
    
        // Start the scheduler
        sched.start()
        .await
        .map_err(|e| ApiError {
            error: crate::error::Error::Generic { description: e.to_string() },
            req_id: ctx.req_id(),
        })?;

        Ok::<(), ApiError>(())
    }).await; 
}