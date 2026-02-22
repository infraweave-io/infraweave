use opentelemetry::trace::TracerProvider as _;
use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::{Config, TracerProvider};
use opentelemetry_sdk::Resource;
use std::time::Duration;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Initialize OpenTelemetry tracing (generic, cloud-agnostic)
/// Exports to any OTLP-compatible backend via environment configuration:
/// - AWS: Set OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 (ADOT Lambda layer)
/// - Azure: Set OTEL_EXPORTER_OTLP_ENDPOINT to Azure Monitor ingestion endpoint
/// - On-prem: Set to Jaeger, Tempo, or other OTLP collector
pub fn init_tracing(service_name: &str) -> anyhow::Result<()> {
    // Get OTLP endpoint from environment, default to localhost for local development
    let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:4317".to_string());

    // Create resource with service information
    let resource = Resource::new(vec![
        KeyValue::new("service.name", service_name.to_string()),
        KeyValue::new("service.version", env!("CARGO_PKG_VERSION").to_string()),
    ]);

    // Create OTLP exporter
    let otlp_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&otlp_endpoint)
        .with_timeout(Duration::from_secs(3))
        .build()?;

    // Create tracer provider with batch span processor
    let tracer_provider = TracerProvider::builder()
        .with_config(Config::default().with_resource(resource))
        .with_batch_exporter(otlp_exporter, opentelemetry_sdk::runtime::Tokio)
        .build();

    // Set global tracer provider
    global::set_tracer_provider(tracer_provider.clone());

    // Create tracing layer
    let tracer = tracer_provider.tracer(service_name.to_string());
    let telemetry_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    // Initialize tracing subscriber with env filter and telemetry layer
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,internal_api=debug"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().with_ansi(false)) // Disable ANSI for CloudWatch
        .with(telemetry_layer)
        .init();

    Ok(())
}

/// Shutdown tracing gracefully
pub fn shutdown_tracing() {
    global::shutdown_tracer_provider();
}
