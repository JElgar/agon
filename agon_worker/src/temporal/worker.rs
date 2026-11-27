//! Temporal worker bootstrap: connect a client, register the workflows +
//! activities, and run the worker poll loop.
//!
//! This runs alongside the SQS consumer in the same binary (spawned as a
//! separate task in `main`), sharing the same `agon_services_core` clients.

use std::net::SocketAddr;
use std::sync::Arc;

use agon_services_core::dao::Dao;
use agon_services_core::opensearch::OpenSearchClient;
use agon_services_core::opensearch::backfill_workflow::{BackfillActivities, BackfillOpenSearch};
use temporalio_client::{
    Client, ClientOptions, Connection, envconfig::LoadClientConfigProfileOptions,
};
use temporalio_common::telemetry::metrics::CoreMeter;
use temporalio_common::telemetry::{
    PrometheusExporterOptions, TelemetryOptions, start_prometheus_metric_exporter,
};
use temporalio_sdk::{Worker, WorkerOptions};
use temporalio_sdk_core::{CoreRuntime, RuntimeOptions};

use super::TASK_QUEUE;
use super::activities::AgonActivities;
use super::workflows::{AcceptInvitation, FanOutMatch, MatchReminder, MaterializeSeries};

/// Where the Temporal SDK's own client/worker metrics (poll latency, task
/// queue backlog, etc.) are served as Prometheus text, for the
/// `temporal-worker` `ServiceMonitor` in `agon_infra` to scrape. Not the
/// `OTEL_EXPORTER_OTLP_ENDPOINT` push path the rest of the binary uses — the
/// Temporal Rust SDK's telemetry only supports a pull-based Prometheus
/// exporter or its own separate OTLP collector config, not our existing OTLP
/// meter provider.
const METRICS_ADDR_ENV: &str = "AGON_WORKER_METRICS_ADDR";
const DEFAULT_METRICS_ADDR: &str = "0.0.0.0:9090";

/// Connect to Temporal (config from the standard `TEMPORAL_*` env / profile) and
/// run the worker until the process exits. Registers every workflow, plus the
/// shared activities struct and (when OpenSearch is configured — see
/// `main.rs`) the OpenSearch backfill's own activities.
pub async fn run(
    dao: Dao,
    opensearch: Option<OpenSearchClient>,
) -> Result<(), Box<dyn std::error::Error>> {
    let metrics_addr: SocketAddr = std::env::var(METRICS_ADDR_ENV)
        .unwrap_or_else(|_| DEFAULT_METRICS_ADDR.to_string())
        .parse()
        .map_err(|e: std::net::AddrParseError| format!("invalid {METRICS_ADDR_ENV}: {e}"))?;
    let prometheus = start_prometheus_metric_exporter(
        PrometheusExporterOptions::builder()
            .socket_addr(metrics_addr)
            .build(),
    )
    .map_err(|e| e.to_string())?;
    tracing::info!(
        addr = %prometheus.bound_addr,
        "temporal worker: serving SDK metrics for Prometheus to scrape"
    );
    let meter: Arc<dyn CoreMeter> = prometheus.meter;

    let telemetry_options = TelemetryOptions::builder().metrics(meter).build();
    let runtime = CoreRuntime::new_assume_tokio(
        RuntimeOptions::builder()
            .telemetry_options(telemetry_options)
            .build()?,
    )?;

    let (conn_options, client_options) =
        ClientOptions::load_from_config(LoadClientConfigProfileOptions::default())?;
    let connection = Connection::connect(conn_options).await?;
    let client = Client::new(connection, client_options)?;

    let mut worker_options = WorkerOptions::new(TASK_QUEUE)
        .register_activities(AgonActivities {
            dao: dao.clone(),
            opensearch: opensearch.clone(),
        })
        .register_workflow::<FanOutMatch>()?
        .register_workflow::<AcceptInvitation>()?
        .register_workflow::<MaterializeSeries>()?
        .register_workflow::<MatchReminder>()?;

    // The backfill workflow needs OpenSearch, which is itself optional (see
    // `main.rs`): without it there's nowhere to backfill into, so the
    // workflow goes unregistered and a start attempt fails at the admin API
    // the same way `POST /admin/opensearch/backfill` already reports
    // OpenSearch being unconfigured elsewhere.
    if let Some(os) = opensearch {
        worker_options = worker_options
            .register_activities(BackfillActivities { dao, os })
            .register_workflow::<BackfillOpenSearch>()?;
    }

    let worker_options = worker_options.build();

    tracing::info!(task_queue = TASK_QUEUE, "temporal worker starting");
    let mut worker = Worker::new(&runtime, client, worker_options)?;
    worker.run().await?;
    Ok(())
}
