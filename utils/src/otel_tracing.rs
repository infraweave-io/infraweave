use opentelemetry::propagation::TextMapPropagator;
use opentelemetry::trace::{SpanId, TraceContextExt, TraceId, TracerProvider as _};
use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{IdGenerator, RandomIdGenerator, TracerProvider};
use opentelemetry_sdk::Resource;
use std::collections::HashMap;
use std::io::IsTerminal;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::fmt::format::{FmtSpan, Writer};
use tracing_subscriber::fmt::time::{FormatTime, SystemTime as FmtSystemTime};
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// The OTLP span exporter, re-exported so callers don't need a direct
/// dependency on `opentelemetry-otlp`.
pub use opentelemetry_otlp::SpanExporter;

/// A span exporter chosen at runtime.
///
/// [`init_tracing`] takes one of these rather than a concrete type so a backend
/// can be anything: OTLP for collectors and vendors, or a cloud-native exporter
/// from the matching `env_*_direct` crate. The SDK has no blanket
/// implementation for `Box<dyn SpanExporter>`, hence the wrapper.
pub struct BoxedSpanExporter(Box<dyn opentelemetry_sdk::export::trace::SpanExporter>);

impl BoxedSpanExporter {
    pub fn new(exporter: impl opentelemetry_sdk::export::trace::SpanExporter + 'static) -> Self {
        Self(Box::new(exporter))
    }
}

impl std::fmt::Debug for BoxedSpanExporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BoxedSpanExporter")
    }
}

impl opentelemetry_sdk::export::trace::SpanExporter for BoxedSpanExporter {
    fn export(
        &mut self,
        batch: Vec<opentelemetry_sdk::export::trace::SpanData>,
    ) -> futures_util::future::BoxFuture<'static, opentelemetry_sdk::export::trace::ExportResult>
    {
        self.0.export(batch)
    }

    fn shutdown(&mut self) {
        self.0.shutdown()
    }

    fn force_flush(
        &mut self,
    ) -> futures_util::future::BoxFuture<'static, opentelemetry_sdk::export::trace::ExportResult>
    {
        self.0.force_flush()
    }

    fn set_resource(&mut self, resource: &Resource) {
        self.0.set_resource(resource)
    }
}

/// Holds the active provider so [`force_flush_tracing`] can drain it between
/// Lambda invocations.
static TRACER_PROVIDER: std::sync::OnceLock<TracerProvider> = std::sync::OnceLock::new();

fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}

/// Resource attributes describing the service and the platform hosting it.
///
/// The platform part is not decoration. CloudWatch Application Signals derives
/// a service's *environment* from these, and with only `service.name` present
/// it files everything under `generic:default` — a different console page from
/// the Lambda or ECS task actually running the code, so the spans arrive,
/// index, and are then effectively invisible in the UI.
///
/// Detection is by the environment variables each runtime sets for itself, so
/// nothing has to be threaded through from the deployment — except
/// `deployment.environment`, which no runtime can know.
fn platform_resource_attributes(service_name: &str) -> Vec<KeyValue> {
    let mut attributes = vec![KeyValue::new("service.name", service_name.to_string())];

    if let Some(environment) = deployment_environment() {
        attributes.push(KeyValue::new("deployment.environment", environment));
    }

    if let Some(version) = service_version() {
        attributes.push(KeyValue::new("service.version", version));
    }

    if let Ok(function_name) = std::env::var("AWS_LAMBDA_FUNCTION_NAME") {
        attributes.push(KeyValue::new("cloud.provider", "aws"));
        attributes.push(KeyValue::new("cloud.platform", "aws_lambda"));
        attributes.push(KeyValue::new("faas.name", function_name));
        if let Ok(version) = std::env::var("AWS_LAMBDA_FUNCTION_VERSION") {
            attributes.push(KeyValue::new("faas.version", version));
        }
    } else if std::env::var_os("ECS_CONTAINER_METADATA_URI_V4").is_some()
        || std::env::var_os("ECS_CONTAINER_METADATA_URI").is_some()
    {
        attributes.push(KeyValue::new("cloud.provider", "aws"));
        attributes.push(KeyValue::new("cloud.platform", "aws_ecs"));
    }

    if let Some(region) = telemetry_aws_region() {
        attributes.push(KeyValue::new("cloud.region", region));
    }

    if let Some(groups) = log_group_names() {
        attributes.push(KeyValue::new(
            "aws.log.group.names",
            opentelemetry::Value::Array(groups.into()),
        ));
    }

    attributes
}

/// The AWS region telemetry is configured for, in the same precedence the X-Ray
/// exporter uses when it picks an endpoint (`env_aws_direct::telemetry`).
///
/// The orders must match. `TELEMETRY_AWS_REGION` exists to override the
/// ambient region, and Lambda always sets `AWS_REGION` — so consulting
/// `AWS_REGION` first would export spans to the overridden region while
/// labelling them `cloud.region` of the other one.
fn telemetry_aws_region() -> Option<String> {
    std::env::var("TELEMETRY_AWS_REGION")
        .or_else(|_| std::env::var("AWS_REGION"))
        .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
        .ok()
}

/// Which deployment this process belongs to, for the `deployment.environment`
/// resource attribute.
///
/// Application Signals builds a service's identity out of name, type and
/// environment, and takes the environment from this attribute. With it unset it
/// falls back to a per-platform default — `lambda:default`, `ecs:default`, or
/// `generic:default` — which is the same string for every account.
///
/// That default is survivable with two accounts and useless with a hundred: the
/// console lists one row per service per account, all named `reconciler`, all
/// reading `lambda:default`, distinguishable only by an account column that is
/// not a metric dimension. A dashboard cannot get at it either. `SEARCH` labels
/// series by dimension, so a hundred accounts sorted worst-first render as a
/// hundred identically labelled rows — correctly ranked and unreadable.
///
/// Set it to something that identifies the deployment rather than the runtime,
/// e.g. `project1-prod`. Left unset the behaviour is unchanged, so this is
/// additive for anything already running.
fn deployment_environment() -> Option<String> {
    let raw = std::env::var("TELEMETRY_ENVIRONMENT").ok()?;
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Which build this process is running, for the `service.version` resource
/// attribute.
///
/// Not derived from `CARGO_PKG_VERSION`: every crate here takes
/// `version.workspace = true`, so that constant is one number for the whole
/// workspace, and inside this crate it resolves to `env_utils` rather than to
/// whatever service is calling. It would name the library, not the deployment.
///
/// The deployment already knows what it pinned — an image digest or tag — so it
/// passes that in. Together with `service.name` and `deployment.environment`
/// this completes the service/env/version triple that vendors key their
/// deployment tracking on, and it is what makes "which build produced this bad
/// span" answerable at all.
///
/// Left unset the attribute is omitted rather than reported as an empty or
/// invented version.
fn service_version() -> Option<String> {
    let raw = std::env::var("TELEMETRY_SERVICE_VERSION").ok()?;
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Log groups to associate the service's spans with, for the Logs tab of a
/// CloudWatch Application Signals service.
///
/// That tab does not search for logs, it follows this attribute. Without it the
/// service page renders with an empty Logs tab however healthy the log group
/// is, because nothing tells the console which group belongs to the service.
///
/// Lambda publishes its own group as an environment variable. ECS only exposes
/// it through the task metadata endpoint, which would mean an HTTP call during
/// startup, so the task definition passes it in instead — it is the same
/// Terraform that declares the group in the first place.
fn log_group_names() -> Option<Vec<opentelemetry::StringValue>> {
    let raw = std::env::var("TELEMETRY_LOG_GROUP_NAMES")
        .or_else(|_| std::env::var("AWS_LAMBDA_LOG_GROUP_NAME"))
        .ok()?;
    let groups: Vec<opentelemetry::StringValue> = raw
        .split(',')
        .map(str::trim)
        .filter(|group| !group.is_empty())
        .map(|group| group.to_string().into())
        .collect();
    (!groups.is_empty()).then_some(groups)
}

fn build_tracer(
    service_name: &str,
    exporter: BoxedSpanExporter,
) -> opentelemetry_sdk::trace::Tracer {
    let resource = Resource::new(platform_resource_attributes(service_name));
    let provider = TracerProvider::builder()
        .with_resource(resource)
        .with_id_generator(XrayIdGenerator::default())
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .build();
    global::set_tracer_provider(provider.clone());
    let _ = TRACER_PROVIDER.set(provider.clone());
    provider.tracer(service_name.to_string())
}

/// Force-flush buffered spans to the exporter. Call this at the end of each
/// Lambda invocation: the execution environment freezes right after the
/// response, so the batch processor would otherwise never push this
/// invocation's spans to the collector/X-Ray before the freeze — which is why
/// Lambda OTel traces silently go missing. No-op when no exporter is active.
pub fn force_flush_tracing() {
    if let Some(provider) = TRACER_PROVIDER.get() {
        for result in provider.force_flush() {
            if let Err(e) = result {
                eprintln!("OpenTelemetry force_flush error: {e:?}");
            }
        }
    }
}

/// ID generator producing AWS X-Ray-compatible trace IDs.
///
/// X-Ray encodes the trace start time in the high 32 bits of the trace ID
/// (`1-<epoch>-<random>`) and drops segments whose timestamp is outside a
/// ~30 day window. OpenTelemetry's default fully-random IDs therefore never
/// show up in X-Ray. This stamps the current epoch into the first 4 bytes and
/// keeps the remaining 96 bits random — still a valid OTel trace ID, so it is
/// safe for non-X-Ray backends too.
#[derive(Clone, Debug, Default)]
struct XrayIdGenerator {
    inner: RandomIdGenerator,
}

impl IdGenerator for XrayIdGenerator {
    fn new_trace_id(&self) -> TraceId {
        let mut bytes = self.inner.new_trace_id().to_bytes();
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as u32)
            .unwrap_or(0);
        bytes[0..4].copy_from_slice(&epoch.to_be_bytes());
        TraceId::from_bytes(bytes)
    }

    fn new_span_id(&self) -> SpanId {
        self.inner.new_span_id()
    }
}

/// Build the generic OTLP/gRPC span exporter pointed at `endpoint`
/// (typically a collector configured via `OTEL_EXPORTER_OTLP_ENDPOINT`).
///
/// This is cloud-agnostic; cloud-specific exporters (e.g. AWS X-Ray with
/// SigV4) live in the respective `env_*_direct` crate.
pub fn otlp_grpc_exporter(endpoint: &str) -> anyhow::Result<BoxedSpanExporter> {
    let exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .with_timeout(Duration::from_secs(3))
        .build()?;
    Ok(BoxedSpanExporter::new(exporter))
}

/// Build the generic OTLP/HTTP span exporter pointed at `endpoint`. Vendor
/// neutral: point it at a local collector/agent (`http://localhost:4318`) or
/// straight at any OTLP/HTTP intake (Datadog, Grafana Cloud, Honeycomb, …).
///
/// Authentication is not handled here — the exporter picks up
/// `OTEL_EXPORTER_OTLP_HEADERS` (or `OTEL_EXPORTER_OTLP_TRACES_HEADERS`) on its
/// own, which is how vendor API keys are supplied.
///
/// `endpoint` must be the full trace URL, because the exporter only appends the
/// `/v1/traces` path to `OTEL_EXPORTER_OTLP_ENDPOINT` — a value passed here is
/// used verbatim. We append it when missing so the localhost default doesn't
/// silently POST to the server root.
///
/// That normalization only ever applies to a caller-supplied endpoint, though:
/// `resolve_http_endpoint` in opentelemetry-otlp returns on
/// `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`, then `OTEL_EXPORTER_OTLP_ENDPOINT`,
/// before it looks at the builder's value at all. So when either is set this
/// argument is dead and the SDK's own path handling decides — including
/// appending `/v1/traces` to an `OTEL_EXPORTER_OTLP_ENDPOINT` that already ends
/// in it. Nothing here can prevent that; use the `_TRACES_` form for a full URL.
pub fn otlp_http_exporter(endpoint: &str) -> anyhow::Result<BoxedSpanExporter> {
    let exporter = SpanExporter::builder()
        .with_http()
        .with_endpoint(normalize_traces_endpoint(endpoint))
        .with_timeout(Duration::from_secs(3))
        .build()?;
    Ok(BoxedSpanExporter::new(exporter))
}

/// Append the OTLP `/v1/traces` signal path unless it is already present.
fn normalize_traces_endpoint(endpoint: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    if trimmed.ends_with("/v1/traces") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1/traces")
    }
}

/// Initialize tracing for a service.
///
/// - Bridges existing `log::*` call sites into `tracing` (default feature
///   on `tracing-subscriber`), so callers don't need to migrate.
/// - Emits minimal plain logs (`<timestamp> <LEVEL> <message>`, no span
///   context/target/ANSI) when `LOG_FORMAT=plain`, JSON logs when
///   `LOG_FORMAT=json`, JSON logs when stderr isn't a TTY, and pretty/ANSI
///   logs otherwise.
/// - When an `exporter` is supplied, spans are batched and exported through it.
///   Selecting/building the exporter (X-Ray, OTLP gRPC, …) is the caller's
///   responsibility; see `env_common`'s telemetry orchestration.
/// - When `exporter` is `None`, tracing/log still works locally without trying
///   to contact a collector.
pub fn init_tracing(service_name: &str, exporter: Option<BoxedSpanExporter>) -> anyhow::Result<()> {
    let telemetry_layer = exporter.map(|exporter| {
        tracing_opentelemetry::layer().with_tracer(build_tracer(service_name, exporter))
    });

    configure_fmt_layer(telemetry_layer, env_filter_from_env())
}

fn env_filter_from_env() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
}

fn configure_fmt_layer<L>(telemetry_layer: Option<L>, env_filter: EnvFilter) -> anyhow::Result<()>
where
    L: tracing_subscriber::Layer<
            tracing_subscriber::layer::Layered<EnvFilter, tracing_subscriber::Registry>,
        > + Send
        + Sync
        + 'static,
{
    let log_format = std::env::var("LOG_FORMAT").ok();
    let stderr_is_tty = std::io::stderr().is_terminal();
    let mode = match log_format.as_deref() {
        Some("plain") => LogMode::Plain,
        Some("json") => LogMode::Json,
        // Default to JSON when stderr isn't a real terminal (files, Docker,
        // collectors); use the human-friendly pretty format interactively.
        _ if stderr_is_tty => LogMode::Pretty,
        _ => LogMode::Json,
    };
    let include_span_context = env_flag("LOG_INCLUDE_SPANS");
    let emit_span_close_events = env_flag("LOG_SPAN_CLOSE_EVENTS");

    let registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(telemetry_layer);

    match mode {
        LogMode::Json => {
            let fmt_layer = tracing_subscriber::fmt::layer()
                .json()
                .flatten_event(true)
                .with_current_span(include_span_context)
                .with_span_list(include_span_context)
                .with_target(true)
                .with_ansi(false)
                .with_span_events(if emit_span_close_events {
                    FmtSpan::CLOSE
                } else {
                    FmtSpan::NONE
                });

            registry.with(fmt_layer).init();
        }
        // Minimal one-liner: timestamp + level + message. Span context (incl.
        // trace_id) and module target are intentionally dropped here since they
        // are available in the exported traces; this keeps log files readable.
        LogMode::Plain => {
            let fmt_layer = tracing_subscriber::fmt::layer()
                .event_format(PlainFormat)
                .with_ansi(false);

            registry.with(fmt_layer).init();
        }
        LogMode::Pretty => {
            registry
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_ansi(true)
                        .with_span_events(FmtSpan::CLOSE),
                )
                .init();
        }
    }

    Ok(())
}

enum LogMode {
    Json,
    Plain,
    Pretty,
}

/// Compact event formatter emitting only `<timestamp> <LEVEL> <message>`.
struct PlainFormat;

impl<S, N> FormatEvent<S, N> for PlainFormat
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        FmtSystemTime.format_time(&mut writer)?;
        write!(writer, " {:>5} ", event.metadata().level())?;
        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

/// Flush and shut down the tracer provider. Call once at process exit.
///
/// The flush is done through our own provider handle on purpose.
/// `global::shutdown_tracer_provider()` sounds like it drains the pipeline, but
/// in opentelemetry 0.27 it only swaps the global for a no-op provider — it
/// exports nothing, and it cannot even drop the real provider while
/// `TRACER_PROVIDER` still holds a clone. Relying on it loses whatever is in
/// the batch queue at exit, which is precisely the process's root span: it
/// closes last, so the batch processor's scheduled flush never comes around
/// again. Long-running services looked fine anyway, because the periodic flush
/// had already shipped every child span — only the entry-point span went
/// missing, and with it the service's entry in CloudWatch Application Signals.
pub fn shutdown_tracing() {
    if let Some(provider) = TRACER_PROVIDER.get() {
        force_flush_tracing();
        if let Err(e) = provider.shutdown() {
            eprintln!("OpenTelemetry shutdown error: {e:?}");
        }
    }
    global::shutdown_tracer_provider();
}

/// Serialize the current span's trace context as a W3C `traceparent` string,
/// for carrying the trace across a process boundary (e.g. into an ECS task's
/// environment when launching the runner). Returns `None` when there is no
/// active, valid trace to propagate.
pub fn current_traceparent() -> Option<String> {
    let cx = tracing::Span::current().context();
    if !cx.span().span_context().is_valid() {
        return None;
    }
    let mut carrier = HashMap::new();
    TraceContextPropagator::new().inject_context(&cx, &mut carrier);
    carrier.remove("traceparent").filter(|tp| !tp.is_empty())
}

/// Adopt the trace context AWS Lambda puts in `_X_AMZN_TRACE_ID` as `span`'s
/// parent, so the service's own spans join the trace Lambda already started
/// rather than beginning a disconnected one.
///
/// Without this a single request produces two unrelated traces: Lambda's
/// built-in active tracing records the invocation under one trace id, and the
/// SDK mints its own for everything underneath. Both show up, neither contains
/// the other, and the durations cannot be lined up.
///
/// The header is X-Ray's own format rather than W3C:
/// `Root=1-<8 hex>-<24 hex>;Parent=<16 hex>;Sampled=<0|1>`, with the fields in
/// any order. No-op when it is absent or unparseable, leaving the span as a
/// root — which is the right outcome outside Lambda.
pub fn set_span_parent_from_xray_header(span: &tracing::Span, header: &str) {
    let Some(span_context) = xray_header_to_span_context(header) else {
        return;
    };
    span.set_parent(opentelemetry::Context::new().with_remote_span_context(span_context));
}

/// The `Root=` trace id from an `_X_AMZN_TRACE_ID` header, in X-Ray's own
/// `1-<8 hex>-<24 hex>` punctuation — the form the console and the `x-trace-id`
/// response header use, so log lines can be correlated with what a caller saw.
///
/// Returns `None` for a header that is absent, malformed, or missing `Root`,
/// rather than passing a half-parsed id on to something that will look it up.
pub fn xray_root_trace_id(header: &str) -> Option<&str> {
    let root = header
        .split(';')
        .filter_map(|field| field.trim().split_once('='))
        .find_map(|(name, value)| (name == "Root").then_some(value))?;
    // Validate through the same parser the span-context path uses, so the two
    // cannot disagree about what counts as a usable header.
    xray_header_to_span_context(header).map(|_| root)
}

/// Parse an `_X_AMZN_TRACE_ID` header into an OpenTelemetry span context.
fn xray_header_to_span_context(header: &str) -> Option<opentelemetry::trace::SpanContext> {
    let mut root = None;
    let mut parent = None;
    let mut sampled = false;

    for field in header.split(';') {
        match field.trim().split_once('=') {
            Some(("Root", value)) => root = Some(value),
            Some(("Parent", value)) => parent = Some(value),
            Some(("Sampled", value)) => sampled = value == "1",
            _ => {}
        }
    }

    // `1-<8 hex>-<24 hex>` holds the same 128 bits as an OTel trace id, just
    // punctuated differently.
    let root = root?;
    let mut parts = root.splitn(3, '-');
    if parts.next()? != "1" {
        return None;
    }
    let (high, low) = (parts.next()?, parts.next()?);
    if high.len() != 8 || low.len() != 24 {
        return None;
    }
    let trace_id = TraceId::from_hex(&format!("{high}{low}")).ok()?;

    // A missing Parent is normal for the first segment in a trace; joining the
    // trace still matters, so fall back to an invalid (absent) parent span.
    let span_id = parent
        .and_then(|p| SpanId::from_hex(p).ok())
        .unwrap_or(SpanId::INVALID);

    let flags = if sampled {
        opentelemetry::trace::TraceFlags::SAMPLED
    } else {
        opentelemetry::trace::TraceFlags::default()
    };

    Some(opentelemetry::trace::SpanContext::new(
        trace_id,
        span_id,
        flags,
        true, // remote
        opentelemetry::trace::TraceState::default(),
    ))
}

/// Set `span`'s parent to the remote trace context encoded in a W3C
/// `traceparent` string (as produced by [`current_traceparent`]), so the span —
/// and everything under it — joins the caller's trace. No-op when the string is
/// empty or invalid, leaving the span with its own freshly generated trace.
pub fn set_span_parent_from_traceparent(span: &tracing::Span, traceparent: &str) {
    if traceparent.trim().is_empty() {
        return;
    }
    let mut carrier = HashMap::new();
    carrier.insert("traceparent".to_string(), traceparent.to_string());
    let cx = TraceContextPropagator::new().extract(&carrier);
    if cx.span().span_context().is_valid() {
        span.set_parent(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A W3C traceparent with a known trace id, from the spec's own example.
    const SAMPLE_TRACEPARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    const SAMPLE_TRACE_ID: &str = "4bf92f3577b34da6a3ce929d0e0e4736";

    /// Run `f` with a subscriber that has a real (non-noop) tracer attached, so
    /// spans get valid OTel contexts. No exporter, so nothing leaves the test.
    fn with_otel_subscriber<T>(f: impl FnOnce() -> T) -> T {
        let provider = TracerProvider::builder()
            .with_id_generator(XrayIdGenerator::default())
            .build();
        let subscriber = tracing_subscriber::registry()
            .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("test")));
        tracing::subscriber::with_default(subscriber, f)
    }

    #[test]
    fn xray_trace_ids_stamp_the_current_epoch_in_the_high_bits() {
        let generator = XrayIdGenerator::default();
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as u32;
        let bytes = generator.new_trace_id().to_bytes();
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as u32;

        let stamped = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        assert!(
            stamped >= before && stamped <= after,
            "expected epoch in {before}..={after}, got {stamped} — X-Ray drops \
             segments whose timestamp is outside a ~30 day window"
        );
    }

    #[test]
    fn xray_trace_ids_stay_valid_and_unique_in_the_low_bits() {
        let generator = XrayIdGenerator::default();
        let a = generator.new_trace_id();
        let b = generator.new_trace_id();

        assert_ne!(a, TraceId::INVALID);
        assert_ne!(b, TraceId::INVALID);
        // Only the first 4 bytes are the timestamp; the remaining 96 bits must
        // still be random or trace ids would collide.
        assert_ne!(a.to_bytes()[4..], b.to_bytes()[4..]);
    }

    #[test]
    fn span_adopts_a_propagated_traceparent() {
        let traceparent = with_otel_subscriber(|| {
            let span = tracing::info_span!("runner");
            set_span_parent_from_traceparent(&span, SAMPLE_TRACEPARENT);
            let _entered = span.enter();
            current_traceparent()
        })
        .expect("an active span should yield a traceparent");

        assert!(
            traceparent.contains(SAMPLE_TRACE_ID),
            "span should have joined the caller's trace, got {traceparent}"
        );
    }

    #[test]
    fn blank_or_malformed_traceparent_leaves_the_span_on_its_own_trace() {
        for input in [
            "",
            "   ",
            "not-a-traceparent",
            "00-invalid-00f067aa0ba902b7-01",
        ] {
            let traceparent = with_otel_subscriber(|| {
                let span = tracing::info_span!("runner");
                set_span_parent_from_traceparent(&span, input);
                let _entered = span.enter();
                current_traceparent()
            });

            if let Some(tp) = traceparent {
                assert!(
                    !tp.contains(SAMPLE_TRACE_ID),
                    "input {input:?} must not adopt a foreign trace"
                );
            }
        }
    }

    #[test]
    fn xray_header_is_adopted_as_the_parent_trace() {
        // Same request, one trace: without this the SDK starts its own trace and
        // Lambda's built-in tracing records the invocation separately.
        let header = "Root=1-6a954a81-14b68dfaffccd7e39d44814f;Parent=53995c3f42cd8ad8;Sampled=1";
        let ctx = xray_header_to_span_context(header).expect("parses");
        assert_eq!(
            ctx.trace_id(),
            TraceId::from_hex("6a954a8114b68dfaffccd7e39d44814f").unwrap()
        );
        assert_eq!(ctx.span_id(), SpanId::from_hex("53995c3f42cd8ad8").unwrap());
        assert!(ctx.is_sampled());
        assert!(ctx.is_remote());
    }

    #[test]
    fn xray_header_fields_may_be_in_any_order_and_parent_is_optional() {
        let ctx = xray_header_to_span_context("Sampled=0;Root=1-6a954a81-14b68dfaffccd7e39d44814f")
            .expect("parses without Parent");
        assert_eq!(ctx.span_id(), SpanId::INVALID);
        assert!(!ctx.is_sampled());
    }

    #[test]
    fn the_root_trace_id_is_extracted_in_xray_punctuation() {
        // This is what goes on the `trace_id` log field and the `x-trace-id`
        // response header, so it must stay in the form the X-Ray console uses
        // rather than the OTel one.
        assert_eq!(
            xray_root_trace_id(
                "Root=1-6a954a81-14b68dfaffccd7e39d44814f;Parent=53995c3f42cd8ad8;Sampled=1"
            ),
            Some("1-6a954a81-14b68dfaffccd7e39d44814f")
        );
        // Field order is not guaranteed; the old `split(';').next()` form only
        // found Root when it happened to come first.
        assert_eq!(
            xray_root_trace_id("Sampled=1;Root=1-6a954a81-14b68dfaffccd7e39d44814f"),
            Some("1-6a954a81-14b68dfaffccd7e39d44814f")
        );
    }

    #[test]
    fn a_malformed_root_yields_no_trace_id_to_correlate_on() {
        for header in ["", "Parent=53995c3f42cd8ad8", "Root=", "Root=1-6a954a81"] {
            assert_eq!(
                xray_root_trace_id(header),
                None,
                "{header:?} should not parse"
            );
        }
    }

    #[test]
    fn malformed_xray_headers_are_ignored_rather_than_guessed_at() {
        for header in [
            "",
            "Root=",
            "Root=2-6a954a81-14b68dfaffccd7e39d44814f", // unknown version
            "Root=1-6a954a81",                          // truncated
            "Root=1-6a954a8-14b68dfaffccd7e39d44814f",  // wrong field widths
            "Root=1-zzzzzzzz-14b68dfaffccd7e39d44814f", // not hex
            "Parent=53995c3f42cd8ad8",                  // no Root
        ] {
            assert!(
                xray_header_to_span_context(header).is_none(),
                "{header:?} should not parse"
            );
        }
    }

    #[test]
    fn span_joins_the_lambda_trace_from_the_header() {
        let traceparent = with_otel_subscriber(|| {
            let span = tracing::info_span!("handler");
            set_span_parent_from_xray_header(
                &span,
                "Root=1-6a954a81-14b68dfaffccd7e39d44814f;Parent=53995c3f42cd8ad8;Sampled=1",
            );
            let _entered = span.enter();
            current_traceparent()
        })
        .expect("an active span yields a traceparent");
        assert!(
            traceparent.contains("6a954a8114b68dfaffccd7e39d44814f"),
            "expected the Lambda trace id, got {traceparent}"
        );
    }

    #[test]
    fn force_flush_without_an_active_provider_is_a_noop() {
        force_flush_tracing();
    }

    #[test]
    fn appends_signal_path_to_a_base_url() {
        assert_eq!(
            normalize_traces_endpoint("http://localhost:4318"),
            "http://localhost:4318/v1/traces"
        );
    }

    #[test]
    fn tolerates_a_trailing_slash() {
        assert_eq!(
            normalize_traces_endpoint("http://localhost:4318/"),
            "http://localhost:4318/v1/traces"
        );
    }

    #[test]
    fn does_not_duplicate_an_existing_signal_path() {
        assert_eq!(
            normalize_traces_endpoint("https://vendor.example/v1/traces"),
            "https://vendor.example/v1/traces"
        );
        assert_eq!(
            normalize_traces_endpoint("https://vendor.example/v1/traces/"),
            "https://vendor.example/v1/traces"
        );
    }

    #[test]
    fn keeps_a_vendor_path_prefix() {
        assert_eq!(
            normalize_traces_endpoint("https://vendor.example/otlp"),
            "https://vendor.example/otlp/v1/traces"
        );
    }
}

#[cfg(test)]
mod platform_tests {
    use super::platform_resource_attributes;
    use std::sync::Mutex;

    // These read process-wide env vars, so they must not run concurrently.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn attrs_with(vars: &[(&str, &str)]) -> Vec<(String, String)> {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for key in [
            "AWS_LAMBDA_FUNCTION_NAME",
            "AWS_LAMBDA_FUNCTION_VERSION",
            "ECS_CONTAINER_METADATA_URI_V4",
            "ECS_CONTAINER_METADATA_URI",
            "AWS_REGION",
            "AWS_DEFAULT_REGION",
            "TELEMETRY_AWS_REGION",
            "TELEMETRY_LOG_GROUP_NAMES",
            "AWS_LAMBDA_LOG_GROUP_NAME",
            "TELEMETRY_ENVIRONMENT",
            "TELEMETRY_SERVICE_VERSION",
        ] {
            std::env::remove_var(key);
        }
        for (k, v) in vars {
            std::env::set_var(k, v);
        }
        let out = platform_resource_attributes("svc")
            .into_iter()
            .map(|kv| (kv.key.to_string(), kv.value.as_str().to_string()))
            .collect();
        for (k, _) in vars {
            std::env::remove_var(k);
        }
        out
    }

    fn get<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
        attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn plain_process_reports_only_the_service() {
        let attrs = attrs_with(&[]);
        assert_eq!(get(&attrs, "service.name"), Some("svc"));
        assert_eq!(get(&attrs, "cloud.platform"), None);
    }

    // Unset is the pre-existing behaviour: Application Signals falls back to its
    // per-platform default, so omitting the attribute has to stay valid rather
    // than emitting an empty string that would read as a real environment.
    #[test]
    fn deployment_environment_is_absent_unless_configured() {
        assert_eq!(get(&attrs_with(&[]), "deployment.environment"), None);
    }

    #[test]
    fn deployment_environment_is_taken_from_the_deployment() {
        let attrs = attrs_with(&[("TELEMETRY_ENVIRONMENT", "project1-prod")]);
        assert_eq!(get(&attrs, "deployment.environment"), Some("project1-prod"));
    }

    // A Terraform variable that is set but empty is the common way this arrives
    // misconfigured, and would otherwise group every such account together under
    // one blank environment.
    #[test]
    fn service_version_is_absent_unless_configured() {
        assert_eq!(get(&attrs_with(&[]), "service.version"), None);
    }

    #[test]
    fn service_version_is_taken_from_the_deployment() {
        let attrs = attrs_with(&[("TELEMETRY_SERVICE_VERSION", "sha256:19c823be")]);
        assert_eq!(get(&attrs, "service.version"), Some("sha256:19c823be"));
    }

    // Absent beats empty: an unset attribute is a gap, whereas an empty one
    // reads as a real version and groups every misconfigured build together.
    #[test]
    fn blank_service_version_is_treated_as_unset() {
        let attrs = attrs_with(&[("TELEMETRY_SERVICE_VERSION", "  ")]);
        assert_eq!(get(&attrs, "service.version"), None);
    }

    #[test]
    fn blank_deployment_environment_is_treated_as_unset() {
        assert_eq!(
            get(
                &attrs_with(&[("TELEMETRY_ENVIRONMENT", "   ")]),
                "deployment.environment"
            ),
            None
        );
    }

    // It describes the deployment, so it has to survive alongside the runtime
    // detection rather than being replaced by the platform default.
    #[test]
    fn deployment_environment_coexists_with_platform_detection() {
        let attrs = attrs_with(&[
            ("TELEMETRY_ENVIRONMENT", "project1-prod"),
            (
                "ECS_CONTAINER_METADATA_URI_V4",
                "http://169.254.170.2/v4/abc",
            ),
        ]);
        assert_eq!(get(&attrs, "deployment.environment"), Some("project1-prod"));
        assert_eq!(get(&attrs, "cloud.platform"), Some("aws_ecs"));
    }

    #[test]
    fn lambda_is_detected_from_its_own_environment() {
        // Without this Application Signals files the service under
        // `generic:default` instead of `lambda:default`, putting its spans on a
        // different console page from the function running them.
        let attrs = attrs_with(&[
            ("AWS_LAMBDA_FUNCTION_NAME", "infraweave-central-api-prod"),
            ("AWS_LAMBDA_FUNCTION_VERSION", "$LATEST"),
            ("AWS_REGION", "us-west-2"),
        ]);
        assert_eq!(get(&attrs, "cloud.platform"), Some("aws_lambda"));
        assert_eq!(get(&attrs, "cloud.provider"), Some("aws"));
        assert_eq!(
            get(&attrs, "faas.name"),
            Some("infraweave-central-api-prod")
        );
        assert_eq!(get(&attrs, "faas.version"), Some("$LATEST"));
        assert_eq!(get(&attrs, "cloud.region"), Some("us-west-2"));
        assert_eq!(get(&attrs, "service.name"), Some("svc"));
    }

    #[test]
    fn ecs_is_detected_and_not_mistaken_for_lambda() {
        let attrs = attrs_with(&[
            (
                "ECS_CONTAINER_METADATA_URI_V4",
                "http://169.254.170.2/v4/abc",
            ),
            ("TELEMETRY_AWS_REGION", "eu-central-1"),
        ]);
        assert_eq!(get(&attrs, "cloud.platform"), Some("aws_ecs"));
        assert_eq!(get(&attrs, "faas.name"), None);
        // ECS does not set AWS_REGION, so the task definition's variable is the
        // only source of region there.
        assert_eq!(get(&attrs, "cloud.region"), Some("eu-central-1"));
    }

    #[test]
    fn an_explicit_telemetry_region_wins_over_the_ambient_one() {
        // Lambda always sets AWS_REGION, so an override that lost to it could
        // never take effect — spans would go to one region (the exporter reads
        // TELEMETRY_AWS_REGION first) and be labelled with another.
        let attrs = attrs_with(&[
            ("AWS_LAMBDA_FUNCTION_NAME", "infraweave-central-api-prod"),
            ("AWS_REGION", "us-west-2"),
            ("TELEMETRY_AWS_REGION", "eu-west-1"),
        ]);
        assert_eq!(get(&attrs, "cloud.region"), Some("eu-west-1"));
    }

    #[test]
    fn aws_default_region_is_the_last_resort() {
        // The exporter accepts it, so the attribute must not go missing when it
        // is the only region in the environment.
        let attrs = attrs_with(&[("AWS_DEFAULT_REGION", "ap-southeast-2")]);
        assert_eq!(get(&attrs, "cloud.region"), Some("ap-southeast-2"));
    }

    #[test]
    fn lambda_contributes_its_own_log_group() {
        // Application Signals' Logs tab follows aws.log.group.names; without it
        // the tab is empty no matter what the group contains.
        let attrs = attrs_with(&[
            ("AWS_LAMBDA_FUNCTION_NAME", "infraweave-reconciler-prod"),
            (
                "AWS_LAMBDA_LOG_GROUP_NAME",
                "/aws/lambda/infraweave-reconciler-prod",
            ),
        ]);
        assert!(get(&attrs, "aws.log.group.names")
            .is_some_and(|v| v.contains("/aws/lambda/infraweave-reconciler-prod")));
    }

    #[test]
    fn ecs_takes_its_log_group_from_the_task_definition() {
        // ECS only exposes the group via the metadata endpoint, so the
        // deployment passes it in rather than the process fetching it.
        let attrs = attrs_with(&[
            (
                "ECS_CONTAINER_METADATA_URI_V4",
                "http://169.254.170.2/v4/abc",
            ),
            (
                "TELEMETRY_LOG_GROUP_NAMES",
                "/infraweave/us-west-2/prod/runner",
            ),
        ]);
        assert!(get(&attrs, "aws.log.group.names")
            .is_some_and(|v| v.contains("/infraweave/us-west-2/prod/runner")));
    }

    #[test]
    fn an_explicit_list_wins_over_the_lambda_default_and_drops_blanks() {
        let attrs = attrs_with(&[
            ("AWS_LAMBDA_LOG_GROUP_NAME", "/aws/lambda/ignored"),
            ("TELEMETRY_LOG_GROUP_NAMES", " /one , ,/two "),
        ]);
        let value = get(&attrs, "aws.log.group.names").expect("groups should be set");
        assert!(value.contains("/one") && value.contains("/two"), "{value}");
        assert!(!value.contains("ignored"), "{value}");
    }

    #[test]
    fn no_log_group_configured_means_no_attribute() {
        assert_eq!(get(&attrs_with(&[]), "aws.log.group.names"), None);
    }
}
