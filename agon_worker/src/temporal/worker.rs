//! Temporal worker bootstrap: connect a client, register the workflows +
//! activities, and run the worker poll loop.
//!
//! ⚠️ UNVERIFIED — see `temporal/mod.rs`. Gated behind the `temporal` feature.
//!
//! This runs alongside the SQS consumer in the same binary (spawned as a
//! separate task in `main`), sharing the same `agon_core` clients.

use agon_core::dao::Dao;
use agon_core::search::SearchClient;
use temporalio_client::{
    Client, ClientOptions, Connection, envconfig::LoadClientConfigProfileOptions,
};
use temporalio_sdk::{Worker, WorkerOptions};
use temporalio_sdk_core::{CoreRuntime, RuntimeOptions};

use super::TASK_QUEUE;
use super::activities::AgonActivities;
use super::workflows::{AcceptInvitation, FanOutMatch};

/// Connect to Temporal (config from the standard `TEMPORAL_*` env / profile) and
/// run the worker until the process exits. Registers both workflows and the
/// shared activities struct.
pub async fn run(dao: Dao, search: SearchClient) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = CoreRuntime::new_assume_tokio(RuntimeOptions::builder().build()?)?;

    let (conn_options, client_options) =
        ClientOptions::load_from_config(LoadClientConfigProfileOptions::default())?;
    let connection = Connection::connect(conn_options).await?;
    let client = Client::new(connection, client_options);

    let worker_options = WorkerOptions::new(TASK_QUEUE)
        .register_activities(AgonActivities { dao, search })
        .register_workflow::<FanOutMatch>()?
        .register_workflow::<AcceptInvitation>()?
        .build();

    tracing::info!(task_queue = TASK_QUEUE, "temporal worker starting");
    Worker::new(&runtime, client, worker_options)?.run().await?;
    Ok(())
}
