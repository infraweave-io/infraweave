use opentelemetry::trace::TracerProvider as _;
use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::TracerProvider;
use opentelemetry_sdk::Resource;
use std::io::IsTerminal;
use std::time::Duration;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Initialize tracing for a service.
///
/// - Bridges existing `log::*` call sites into `tracing` (default feature
///   on `tracing-subscriber`), so callers don't need to migrate.
/// - Emits JSON to stderr when `LOG_FORMAT=json` or stderr isn't a TTY,
///   pretty/ANSI otherwise.
/// - If an OTLP exporter can be built (default endpoint
///   `http://localhost:4317`, override via `OTEL_EXPORTER_OTLP_ENDPOINT`),
///   spans are exported. Otherwise tracing/log still works locally — the
///   batch exporter swallows transient connectivity errors at runtime.
pub fn init_tracing(service_name: &str) -> anyhow::Result<()> {
    let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:4317".to_string());

    let resource = Resource::new(vec![KeyValue::new(
        "service.name",
        service_name.to_string(),
    )]);

    let telemetry_layer = match opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&otlp_endpoint)
        .with_timeout(Duration::from_secs(3))
        .build()
    {
        Ok(exporter) => {
            let provider = TracerProvider::builder()
                .with_resource(resource)
                .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
                .build();
            global::set_tracer_provider(provider.clone());
            let tracer = provider.tracer(service_name.to_string());
            Some(tracing_opentelemetry::layer().with_tracer(tracer))
        }
        Err(e) => {
            eprintln!(
                "OpenTelemetry exporter init failed ({}); continuing without OTLP export",
                e
            );
            None
        }
    };

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let json =
        std::env::var("LOG_FORMAT").as_deref() == Ok("json") || !std::io::stderr().is_terminal();

    let registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(telemetry_layer);

    if json {
        registry
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .flatten_event(true)
                    // Include the full span chain so fields set on the root
                    // span (e.g. `trace_id`) appear on every event emitted
                    // inside nested spans.
                    .with_current_span(true)
                    .with_span_list(true)
                    .with_target(true)
                    .with_ansi(false)
                    // Emit one event per span close, with `time.busy` /
                    // `time.idle` timing — gives you a duration log line
                    // for every instrumented function.
                    .with_span_events(FmtSpan::CLOSE),
            )
            .init();
    } else {
        registry
            .with(
                tracing_subscriber::fmt::layer()
                    .with_ansi(true)
                    .with_span_events(FmtSpan::CLOSE),
            )
            .init();
    }

    Ok(())
}

/// Flush and shut down the global tracer provider. Call once at process exit.
pub fn shutdown_tracing() {
    global::shutdown_tracer_provider();
}
