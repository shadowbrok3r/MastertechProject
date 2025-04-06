use database::schema::prestashop_schema::Employee;
use tokio_cron_scheduler::{Job, JobScheduler};
use crate::AppState;

use super::data::get_services_by_status;

pub async fn start_cron_job(
    state: AppState, 
    id: String,
    endpoint: String, 
    employee: Employee
) 
    -> anyhow::Result<(), anyhow::Error> 
{
    let sched = JobScheduler::new().await?;

    // Schedule a job to run at 2 AM every night ("0 0 2 * * * *")
    sched.add(
                                // Every two minutes just as a test
        Job::new_async("0 */2 * * * *", move |_uuid, _l| {
            let state = state.clone();
            let endpoint = endpoint.clone();
            let store = employee.id_store.clone();
            let id = id.clone();
            Box::pin(async move {
                let mut cache = state.cache.lock().await;

                // Fetch services within the range
                // Handle the fetched services
                match get_services_by_status(&id, &store).await {
                    Ok(svcs) => { cache.insert(endpoint, crate::CachedData { orders: svcs.clone() }); },
                    Err(e) => println!("Error getting in repair shelf services: {:?}", e)
                };

            })
        })?
    ).await?;

    sched.start().await?;

    Ok(())
}
