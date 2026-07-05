//! OpenTelemetry wiring for the whole workspace.
//!
//! Both binaries (`agon_service`, `agon_worker`) call [`init`] at startup. It
//! stands up the three OTLP signal pipelines — traces, metrics and logs — and
//! bridges the existing `tracing` instrumentation onto them:
//!
//!   * **traces** — `tracing` spans become OTel spans via `tracing-opentelemetry`.
//!   * **logs**   — `tracing` events become OTel log records via
//!     `opentelemetry-appender-tracing`.
//!   * **metrics** — a global meter is registered so app code (e.g. the API
//!     request middleware) can record instruments; they export periodically.
//!
//! Everything is pushed over OTLP/gRPC to a single collector (Grafana Alloy in
//! the cluster; see docs/observability.md), which fans out to Tempo / Loki /
//! Prometheus. The endpoint and service name come from the standard OTel env
//! vars, so nothing here is agon-specific:
//!
//!   * `OTEL_EXPORTER_OTLP_ENDPOINT` — e.g. `http://alloy.observability:4317`.
//!     When **unset**, export is disabled entirely and we fall back to plain
//!     JSON logs on stdout — so local dev and tests need no collector.
//!   * `OTEL_SERVICE_NAME` — logical service name (`agon-service` / `agon-worker`).
//!   * `RUST_LOG` / `OTEL_LOG_LEVEL` — the `tracing` env filter (default `info`).
//!
//! [`init`] returns a [`Telemetry`] guard; hold it for the process lifetime and
//! let it drop on shutdown so the batch exporters flush.

use std::time::Duration;

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

/// Env var naming the OTLP collector endpoint. Absent ⇒ exporters disabled.
const OTLP_ENDPOINT_ENV: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";

/// Held for the process lifetime; dropping it flushes and shuts the exporters
/// down cleanly so no buffered spans/logs/metrics are lost on exit.
///
/// When export is disabled (no OTLP endpoint configured) the provider fields
/// are `None` and dropping is a no-op.
#[must_use = "hold the guard for the process lifetime; dropping it early stops telemetry export"]
pub struct Telemetry {
    tracer_provider: Option<SdkTracerProvider>,
    meter_provider: Option<SdkMeterProvider>,
    logger_provider: Option<SdkLoggerProvider>,
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        // Best-effort flush; errors here only matter at shutdown and there's
        // nothing actionable to do with them.
        if let Some(p) = self.tracer_provider.take() {
            let _ = p.shutdown();
        }
        if let Some(p) = self.meter_provider.take() {
            let _ = p.shutdown();
        }
        if let Some(p) = self.logger_provider.take() {
            let _ = p.shutdown();
        }
    }
}

/// Initialise tracing + OpenTelemetry export for `service_name`.
///
/// Must be called once, early in `main`, from within a Tokio runtime (the OTLP
/// tonic exporters require one). The `service_name` is the fallback for the
/// `service.name` resource attribute when `OTEL_SERVICE_NAME` isn't set.
///
/// If `OTEL_EXPORTER_OTLP_ENDPOINT` is unset, only the JSON stdout logger is
/// installed and the returned guard is inert — so local runs and tests behave
/// exactly as before this module existed.
pub fn init(service_name: &'static str) -> Telemetry {
    let filter = || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // Always emit JSON logs to stdout. In the cluster these are still captured
    // (Alloy tails pod stdout as a backstop); locally they're the whole story.
    let fmt_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_filter(filter());

    let endpoint = std::env::var(OTLP_ENDPOINT_ENV)
        .ok()
        .filter(|e| !e.trim().is_empty());

    let Some(endpoint) = endpoint else {
        // No collector configured — stdout logs only.
        tracing_subscriber::registry().with(fmt_layer).init();
        tracing::info!(
            service.name = service_name,
            "telemetry: {OTLP_ENDPOINT_ENV} unset; OTLP export disabled (stdout logs only)"
        );
        return Telemetry {
            tracer_provider: None,
            meter_provider: None,
            logger_provider: None,
        };
    };

    let resource = Resource::builder().with_service_name(service_name).build();
    let timeout = Duration::from_secs(5);

    // ── Traces ──────────────────────────────────────────────────────────────
    let span_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .with_timeout(timeout)
        .build()
        .expect("failed to build OTLP span exporter");
    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_batch_exporter(span_exporter)
        .build();

    // ── Metrics ─────────────────────────────────────────────────────────────
    let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .with_timeout(timeout)
        .build()
        .expect("failed to build OTLP metric exporter");
    let meter_provider = SdkMeterProvider::builder()
        .with_resource(resource.clone())
        .with_periodic_exporter(metric_exporter)
        .build();

    // ── Logs ────────────────────────────────────────────────────────────────
    let log_exporter = opentelemetry_otlp::LogExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .with_timeout(timeout)
        .build()
        .expect("failed to build OTLP log exporter");
    let logger_provider = SdkLoggerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(log_exporter)
        .build();

    // Register the meter + tracer providers globally so `opentelemetry::global`
    // (used by the metrics middleware and any manual spans) resolves to them.
    opentelemetry::global::set_tracer_provider(tracer_provider.clone());
    opentelemetry::global::set_meter_provider(meter_provider.clone());

    // Bridge `tracing` → OTel: spans via the tracing-opentelemetry layer, and
    // log events via the appender bridge. Each layer gets its own env filter.
    let tracer = tracer_provider.tracer(service_name);
    let otel_trace_layer = tracing_opentelemetry::layer()
        .with_tracer(tracer)
        .with_filter(filter());
    let otel_log_layer =
        opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&logger_provider)
            .with_filter(filter());

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(otel_trace_layer)
        .with(otel_log_layer)
        .init();

    tracing::info!(
        service.name = service_name,
        endpoint = %endpoint,
        "telemetry: OTLP export enabled (traces, metrics, logs)"
    );

    Telemetry {
        tracer_provider: Some(tracer_provider),
        meter_provider: Some(meter_provider),
        logger_provider: Some(logger_provider),
    }
}
