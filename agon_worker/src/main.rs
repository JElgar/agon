//! agon_worker — the async processing worker.
//!
//! Long-polls the SQS events queue (fed by DynamoDB Streams via an EventBridge
//! Pipe). The main consumer (`consumer.rs`) doesn't run any handler itself: for
//! each event it fans out onto the shared `agon-actions` queue (one message
//! per `action::Action` — indexing, notifications, push, stats, tournament,
//! rating) and, if OpenSearch syncing applies, onto its own dedicated queue
//! (`opensearch_sync`). `action_consumer::ActionConsumer` and
//! `opensearch_sync::SyncConsumer` are the independent poll loops that
//! actually run those handlers, each with its own retry/DLQ, so a failure in
//! one can never hold up or replay another. See docs/opensearch.md for why
//! OpenSearch gets a queue of its own instead of sharing `agon-actions`.
//!
//! Multi-step orchestration (feed fan-out, the accept-invitation saga) lives
//! in the `temporal` module and runs a Temporal worker alongside the consumer
//! loops in this same binary.
//!
//! `agon_worker backfill-opensearch [...]` runs the OpenSearch backfill instead
//! of the worker and exits (see `backfill`).

mod action;
mod action_consumer;
mod asset_consumer;
mod backfill;
mod config;
mod consumer;
mod error;
mod event;
mod handlers;
mod opensearch_sync;
mod temporal;

use agon_services_core::dao::Dao;
use aws_sdk_sqs::Client as SqsClient;

use crate::action_consumer::{ActionConsumer, ActionProducer};
use crate::config::Config;
use crate::consumer::Consumer;
use agon_services_core::push::PushClient;
use agon_services_core::search::SearchClient;

#[tokio::main]
async fn main() {
    // Held for the process lifetime; dropping it on the way out of `main`
    // flushes the OTLP batch exporters. See agon_services_core::telemetry.
    let _telemetry = agon_services_core::telemetry::init("agon-worker");

    // One-off jobs share the binary (and its image) but not the worker's config.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("backfill-opensearch") {
        let code = match backfill::BackfillArgs::parse(&args[1..]) {
            Ok(a) => backfill::run(a).await,
            Err(e) => {
                tracing::error!(error = %e, "invalid backfill arguments");
                2
            }
        };
        drop(_telemetry);
        std::process::exit(code);
    }

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
        Some(json) => match PushClient::new(json).await {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::error!(error = %e, "failed to build FCM push client; exiting");
                std::process::exit(1);
            }
        },
    };

    // The asset consumer shares the SQS client, DAO and config; build it before
    // moving `config` into the main consumer.
    let asset_consumer = asset_consumer::AssetConsumer::new(
        sqs.clone(),
        dao.clone(),
        std::sync::Arc::new(config.clone()),
    );

    let config_arc = std::sync::Arc::new(config.clone());
    let action_consumer = ActionConsumer::new(
        sqs.clone(),
        dao.clone(),
        search.clone(),
        push,
        config.actions_queue_url.clone(),
        config_arc.clone(),
    );
    let actions = ActionProducer::new(sqs.clone(), config.actions_queue_url.clone());

    // Captured before `sqs`/`config` move into the main consumer below; used
    // by the OpenSearch sync consumer, which is a second, independent poll
    // loop over its own queue.
    let sync_sqs = sqs.clone();
    let opensearch_sync_queue_url = config.opensearch_sync_queue_url.clone();

    let mut consumer = Consumer::new(sqs, actions, config);

    // OpenSearch: opt-in via OPENSEARCH_URL. Unlike Meilisearch, a failure here
    // is not fatal, since nothing reads OpenSearch yet; the worker runs
    // without it and the backfill can catch up later.
    let opensearch =
        agon_services_core::opensearch::OpenSearchClient::from_env().unwrap_or_else(|e| {
            tracing::error!(error = %e, "invalid OpenSearch settings; syncing disabled");
            None
        });
    // Also handed to the Temporal worker (for the backfill workflow's
    // activities) and to the sync consumer below — kept outside the `if let`
    // so it survives the clone each of those takes its own copy from.
    let mut opensearch_ready = None;
    if let Some(os) = opensearch {
        let mut ready = true;
        for def in agon_services_core::opensearch::all_indices() {
            if let Err(e) = os.ensure_index(&def).await {
                tracing::error!(index = %os.write_index(&def), error = %e, "opensearch index setup failed; syncing disabled");
                ready = false;
                break;
            }
        }
        if ready {
            tracing::info!("opensearch syncing enabled");
            opensearch_ready = Some(os);
        }
    } else {
        tracing::info!("OPENSEARCH_URL unset; opensearch syncing disabled");
    }

    // Sending/consuming OpenSearch sync targets: needs both the client above
    // and AGON_OPENSEARCH_SYNC_QUEUE_URL. Either missing just means OpenSearch
    // syncing is off, same as any other optional integration.
    let sync_consumer = match (opensearch_ready.clone(), opensearch_sync_queue_url) {
        (Some(os), Some(queue_url)) => {
            consumer = consumer.with_opensearch_sync(opensearch_sync::SyncProducer::new(
                sync_sqs.clone(),
                queue_url.clone(),
            ));
            Some(opensearch_sync::SyncConsumer::new(
                sync_sqs,
                dao.clone(),
                os,
                queue_url,
                config_arc,
            ))
        }
        (os, queue_url) => {
            if os.is_some() && queue_url.is_none() {
                tracing::info!("AGON_OPENSEARCH_SYNC_QUEUE_URL unset; opensearch syncing disabled");
            }
            None
        }
    };

    // Attach a client so multi-step stream events start workflows. A connection
    // failure here is fatal — Temporal is a required dependency of the worker.
    let consumer = match temporal::client::TemporalClient::connect().await {
        Ok(client) => consumer.with_temporal(client),
        Err(e) => {
            tracing::error!(error = %e, "failed to connect Temporal client; exiting");
            std::process::exit(1);
        }
    };

    // Run every consumer AND the Temporal worker concurrently. The Temporal
    // worker's futures are `!Send` (workflows run single-threaded by design,
    // using Rc/RefCell internally), so it cannot be `tokio::spawn`ed — `join!`
    // polls all of them on this same task, which doesn't require `Send`. Every
    // consumer observes the same shutdown signal (fanned out below).
    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);
    let events_shutdown = subscribe_shutdown(&shutdown_tx);
    let asset_shutdown = subscribe_shutdown(&shutdown_tx);
    let action_shutdown = subscribe_shutdown(&shutdown_tx);
    let sync_shutdown = subscribe_shutdown(&shutdown_tx);
    let signal_fut = async move {
        shutdown_signal().await;
        // Best-effort broadcast; receivers stop on the first (or a lagged) recv.
        let _ = shutdown_tx.send(());
    };

    let consumer_fut = consumer.run(Box::pin(events_shutdown));
    let asset_fut = asset_consumer.run(Box::pin(asset_shutdown));
    let action_fut = action_consumer.run(Box::pin(action_shutdown));
    // Resolves immediately (a no-op) if OpenSearch syncing isn't configured;
    // the other futures keep the process alive regardless.
    let sync_fut = async {
        if let Some(sync_consumer) = sync_consumer {
            sync_consumer.run(Box::pin(sync_shutdown)).await;
        }
    };
    let temporal_fut = async {
        if let Err(e) = temporal::worker::run(dao, search, opensearch_ready).await {
            tracing::error!(error = %e, "temporal worker exited with error");
        }
    };
    tokio::join!(
        signal_fut,
        consumer_fut,
        asset_fut,
        action_fut,
        sync_fut,
        temporal_fut
    );

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
