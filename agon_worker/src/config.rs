//! Worker configuration, read from the environment (injected by the k8s
//! Deployment via the `aws-credentials` secret and worker-specific env).

use std::env;

use crate::error::{WorkerError, WorkerResult};

/// All runtime configuration for the worker.
#[derive(Debug, Clone)]
pub struct Config {
    /// DynamoDB single-table name (`AGON_TABLE_NAME`).
    pub table_name: String,
    /// SQS queue URL for change events off the DynamoDB stream, long-polled by the
    /// main consumer (`AGON_EVENTS_QUEUE_URL`).
    pub events_queue_url: String,
    /// SQS queue URL for S3 asset-upload events, long-polled by the asset consumer
    /// (`AGON_ASSET_EVENTS_QUEUE_URL`).
    pub asset_events_queue_url: String,
    /// CloudFront base URL used to build the canonical serving URL stored on an
    /// asset when it uploads (`AGON_ASSETS_CDN_URL`).
    pub assets_cdn_url: String,
    /// Base URL of the Meilisearch instance (`MEILI_URL`).
    pub meili_url: String,
    /// Meilisearch API key (`MEILI_MASTER_KEY`).
    pub meili_key: String,
    /// Max messages to pull per SQS receive (1..=10).
    pub batch_size: i32,
    /// SQS long-poll wait time in seconds (0..=20).
    pub wait_time_seconds: i32,
    /// Visibility timeout for in-flight messages, in seconds.
    pub visibility_timeout_seconds: i32,
}

impl Config {
    /// Load configuration from the environment, failing if a required var is
    /// missing.
    pub fn from_env() -> WorkerResult<Self> {
        Ok(Self {
            table_name: required("AGON_TABLE_NAME")?,
            events_queue_url: required("AGON_EVENTS_QUEUE_URL")?,
            asset_events_queue_url: required("AGON_ASSET_EVENTS_QUEUE_URL")?,
            assets_cdn_url: required("AGON_ASSETS_CDN_URL")?
                .trim_end_matches('/')
                .to_string(),
            meili_url: required("MEILI_URL")?,
            meili_key: required("MEILI_MASTER_KEY")?,
            batch_size: optional_parsed("AGON_WORKER_BATCH_SIZE", 10)?,
            wait_time_seconds: optional_parsed("AGON_WORKER_WAIT_SECONDS", 20)?,
            visibility_timeout_seconds: optional_parsed("AGON_WORKER_VISIBILITY_SECONDS", 60)?,
        })
    }
}

fn required(key: &str) -> WorkerResult<String> {
    env::var(key).map_err(|_| WorkerError::Config(format!("missing env var `{key}`")))
}

fn optional_parsed<T: std::str::FromStr>(key: &str, default: T) -> WorkerResult<T> {
    match env::var(key) {
        Ok(v) => v
            .parse()
            .map_err(|_| WorkerError::Config(format!("env var `{key}` is not a valid value"))),
        Err(_) => Ok(default),
    }
}
