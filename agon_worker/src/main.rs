//! agon_worker — the async processing worker.
//!
//! Long-polls the SQS events queue (fed by DynamoDB Streams via an EventBridge
//! Pipe) and runs inline handlers: search indexing (Meilisearch) and
//! notification generation. See docs/async-design.md.
//!
//! The inline slice always runs. Multi-step orchestration (feed fan-out, the
//! accept-invitation saga) lives in the `temporal` module behind the
//! off-by-default `temporal` feature; when enabled it runs a Temporal worker
//! alongside the consumer loop in this same binary.

mod config;
mod consumer;
mod error;
mod event;
mod handlers;
#[cfg(feature = "temporal")]
mod temporal;

use agon_core::dao::Dao;
use aws_sdk_sqs::Client as SqsClient;

use crate::config::Config;
use crate::consumer::Consumer;
use agon_core::search::SearchClient;

#[tokio::main]
async fn main() {
    init_tracing();

    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "invalid configuration; exiting");
            std::process::exit(1);
        }
    };

    let aws_config = aws_config::load_from_env().await;
    let sqs = SqsClient::new(&aws_config);
    let dao = Dao::from_env(config.table_name.clone()).await;
    let search = SearchClient::new(config.meili_url.clone(), config.meili_key.clone());

    // Ensure the search indexes exist with the right filterable / sortable
    // attributes before we start indexing off the stream. Idempotent, so safe on
    // every start; fatal if it fails, since indexing would otherwise silently
    // produce an unqueryable index.
    if let Err(e) = search.bootstrap().await {
        tracing::error!(error = %e, "failed to configure search indexes; exiting");
        std::process::exit(1);
    }

    let consumer = Consumer::new(sqs, dao.clone(), search.clone(), config);

    // With the `temporal` feature, attach a client so multi-step stream events
    // start workflows. A connection failure here is fatal (the feature was
    // explicitly enabled, so Temporal is expected to be reachable).
    #[cfg(feature = "temporal")]
    let consumer = match temporal::client::TemporalClient::connect().await {
        Ok(client) => consumer.with_temporal(client),
        Err(e) => {
            tracing::error!(error = %e, "failed to connect Temporal client; exiting");
            std::process::exit(1);
        }
    };

    // Without Temporal: just run the SQS consumer until shutdown.
    #[cfg(not(feature = "temporal"))]
    consumer.run(Box::pin(shutdown_signal())).await;

    // With Temporal: run the SQS consumer AND the Temporal worker concurrently.
    // The Temporal worker's futures are `!Send` (workflows run single-threaded
    // by design, using Rc/RefCell internally), so it cannot be `tokio::spawn`ed
    // — `join!` polls both on this same task, which doesn't require `Send`.
    #[cfg(feature = "temporal")]
    {
        let consumer_fut = consumer.run(Box::pin(shutdown_signal()));
        let temporal_fut = async {
            if let Err(e) = temporal::worker::run(dao, search).await {
                tracing::error!(error = %e, "temporal worker exited with error");
            }
        };
        tokio::join!(consumer_fut, temporal_fut);
    }

    tracing::info!("worker stopped");
}

fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().json().with_env_filter(filter).init();
}

/// Resolves on SIGTERM (k8s pod termination) or Ctrl-C, so in-flight messages
/// aren't cut off mid-process and the loop stops cleanly.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => tracing::error!(error = %e, "failed to install SIGTERM handler"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
