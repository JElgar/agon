//! agon_worker — the async processing worker.
//!
//! Long-polls the SQS events queue (fed by DynamoDB Streams via an EventBridge
//! Pipe) and runs inline handlers: search indexing (Meilisearch) and
//! notification generation. See docs/async-design.md.
//!
//! The inline slice always runs. Multi-step orchestration (feed fan-out, the
//! accept-invitation saga) lives in the `temporal` module and runs a Temporal
//! worker alongside the consumer loop in this same binary.

mod asset_consumer;
mod config;
mod consumer;
mod error;
mod event;
mod handlers;
mod temporal;

use agon_core::dao::Dao;
use aws_sdk_sqs::Client as SqsClient;

use crate::config::Config;
use crate::consumer::Consumer;
use agon_core::push::{PushClient, ServiceAccountJson};
use agon_core::search::SearchClient;

#[tokio::main]
async fn main() {
    // Held for the process lifetime; dropping it on the way out of `main`
    // flushes the OTLP batch exporters. See agon_core::telemetry.
    let _telemetry = agon_core::telemetry::init("agon-worker");

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

    // Push is opt-in: absent config means "not configured" (e.g. local dev),
    // not a failure. Present-but-invalid is a real misconfiguration, so that
    // still exits.
    let push = match &config.fcm_service_account_json {
        None => {
            tracing::info!("AGON_FCM_SERVICE_ACCOUNT_JSON unset; push notifications disabled");
            None
        }
        Some(json) => {
            let service_account: ServiceAccountJson = match serde_json::from_str(json) {
                Ok(sa) => sa,
                Err(e) => {
                    tracing::error!(error = %e, "invalid AGON_FCM_SERVICE_ACCOUNT_JSON; exiting");
                    std::process::exit(1);
                }
            };
            match PushClient::new(service_account) {
                Ok(p) => Some(p),
                Err(e) => {
                    tracing::error!(error = %e, "failed to build FCM push client; exiting");
                    std::process::exit(1);
                }
            }
        }
    };

    // The asset consumer shares the SQS client, DAO and config; build it before
    // moving `config` into the main consumer.
    let asset_consumer = asset_consumer::AssetConsumer::new(
        sqs.clone(),
        dao.clone(),
        std::sync::Arc::new(config.clone()),
    );

    let consumer = Consumer::new(sqs, dao.clone(), search.clone(), push, config);

    // Attach a client so multi-step stream events start workflows. A connection
    // failure here is fatal — Temporal is a required dependency of the worker.
    let consumer = match temporal::client::TemporalClient::connect().await {
        Ok(client) => consumer.with_temporal(client),
        Err(e) => {
            tracing::error!(error = %e, "failed to connect Temporal client; exiting");
            std::process::exit(1);
        }
    };

    // Run the SQS consumers AND the Temporal worker concurrently. The Temporal
    // worker's futures are `!Send` (workflows run single-threaded by design,
    // using Rc/RefCell internally), so it cannot be `tokio::spawn`ed — `join!`
    // polls all of them on this same task, which doesn't require `Send`. Both
    // consumers observe the same shutdown signal (fanned out below).
    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);
    let events_shutdown = subscribe_shutdown(&shutdown_tx);
    let asset_shutdown = subscribe_shutdown(&shutdown_tx);
    let signal_fut = async move {
        shutdown_signal().await;
        // Best-effort broadcast; receivers stop on the first (or a lagged) recv.
        let _ = shutdown_tx.send(());
    };

    let consumer_fut = consumer.run(Box::pin(events_shutdown));
    let asset_fut = asset_consumer.run(Box::pin(asset_shutdown));
    let temporal_fut = async {
        if let Err(e) = temporal::worker::run(dao, search).await {
            tracing::error!(error = %e, "temporal worker exited with error");
        }
    };
    tokio::join!(signal_fut, consumer_fut, asset_fut, temporal_fut);

    tracing::info!("worker stopped");
}

/// A future that resolves when the shutdown broadcast fires (or the sender is
/// dropped / the receiver lags), used to stop each consumer loop cleanly.
fn subscribe_shutdown(
    tx: &tokio::sync::broadcast::Sender<()>,
) -> impl std::future::Future<Output = ()> + use<> {
    let mut rx = tx.subscribe();
    async move {
        let _ = rx.recv().await;
    }
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
