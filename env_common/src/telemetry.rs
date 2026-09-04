//! Telemetry orchestration: selects a span exporter based on the
//! `TELEMETRY_EXPORTER` environment variable and hands it to the generic
//! tracing setup in `env_utils`. Cloud-specific exporters live in the
//! respective `env_*_direct` crate (e.g. AWS X-Ray in `env_aws_direct`).

use env_utils::otel_tracing::{self, BoxedSpanExporter};

/// Conventional local collector endpoint, used when `otlp-http` is selected
/// without an explicit `OTEL_EXPORTER_OTLP_ENDPOINT`.
const DEFAULT_OTLP_HTTP_ENDPOINT: &str = "http://localhost:4318";

/// Which backend the configuration resolves to, before any exporter is built.
/// Separated from [`select_exporter`] so the dispatch can be tested without
/// touching process-wide environment variables or contacting a backend.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ExporterChoice {
    /// No span export; logging only.
    Disabled,
    /// AWS X-Ray via its OTLP endpoint.
    Xray,
    /// OTLP/HTTP to the given endpoint.
    OtlpHttp(String),
    /// OTLP/gRPC to the given endpoint.
    OtlpGrpc(String),
}

/// Trim a setting, treating whitespace-only as absent.
fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// Map `TELEMETRY_EXPORTER` (and the OTLP endpoints, where relevant) onto a
/// backend choice. Anything unrecognised, blank, or missing a required endpoint
/// degrades to [`ExporterChoice::Disabled`] with a warning, so a telemetry
/// misconfiguration never stops a service from starting.
fn choose_exporter(
    setting: Option<&str>,
    otlp_endpoint: Option<&str>,
    otlp_traces_endpoint: Option<&str>,
) -> ExporterChoice {
    let endpoint = non_blank(otlp_endpoint);

    match non_blank(setting) {
        // `aws` and `xray-otlp` are retained as aliases so deployments that
        // predate the rename keep working.
        Some("xray") | Some("xray-otlp") | Some("aws") => {
            // opentelemetry-otlp resolves OTEL_EXPORTER_OTLP_(TRACES_)ENDPOINT
            // ahead of the endpoint handed to the builder, so leaving either set
            // does not merely redirect the spans: the requests are still signed
            // with SigV4 for the X-Ray host, and the third party receives an
            // Authorization header naming our access key. Refuse rather than
            // export, since the spans would not reach X-Ray either way.
            if let Some(redirect) = endpoint.or(non_blank(otlp_traces_endpoint)) {
                eprintln!(
                    "TELEMETRY_EXPORTER=xray but OTEL_EXPORTER_OTLP_ENDPOINT / \
                     OTEL_EXPORTER_OTLP_TRACES_ENDPOINT is set ({redirect}); that endpoint \
                     takes precedence over the X-Ray one, so SigV4-signed requests would go \
                     there instead. Continuing without OTLP export — unset it, or select \
                     TELEMETRY_EXPORTER=otlp-http to use it deliberately"
                );
                return ExporterChoice::Disabled;
            }
            ExporterChoice::Xray
        }
        // Defaults to the conventional local collector endpoint so a sidecar or
        // agent needs no extra configuration.
        Some("otlp-http") => {
            ExporterChoice::OtlpHttp(endpoint.unwrap_or(DEFAULT_OTLP_HTTP_ENDPOINT).to_string())
        }
        Some("otlp-grpc") => match endpoint {
            Some(endpoint) => ExporterChoice::OtlpGrpc(endpoint.to_string()),
            None => {
                eprintln!(
                    "TELEMETRY_EXPORTER=otlp-grpc requires OTEL_EXPORTER_OTLP_ENDPOINT; continuing without OTLP export"
                );
                ExporterChoice::Disabled
            }
        },
        Some("none") | None => ExporterChoice::Disabled,
        Some(other) => {
            eprintln!("Unknown TELEMETRY_EXPORTER value ({other}); continuing without OTLP export");
            ExporterChoice::Disabled
        }
    }
}

/// Resolve the configured span exporter, if any.
///
/// - `TELEMETRY_EXPORTER=xray` → AWS X-Ray via its OTLP endpoint. Requires
///   X-Ray Transaction Search on the account; `xray-otlp` and `aws` are
///   accepted as aliases. Refuses to export if an `OTEL_EXPORTER_OTLP_*`
///   endpoint is also set, since that would take precedence.
/// - `TELEMETRY_EXPORTER=otlp-http` → OTLP/HTTP to `OTEL_EXPORTER_OTLP_ENDPOINT`
///   (default `http://localhost:4318`).
/// - `TELEMETRY_EXPORTER=otlp-grpc` → OTLP/gRPC to `OTEL_EXPORTER_OTLP_ENDPOINT`.
/// - unset / `none` / unknown → no exporter (local logging only).
///
/// The two `otlp-*` modes are vendor neutral: point them at a collector, a
/// vendor agent, or a hosted OTLP intake (Datadog, Grafana Cloud, Honeycomb, …).
/// API keys travel via the standard `OTEL_EXPORTER_OTLP_HEADERS`, which the
/// exporter reads directly.
///
/// Failures to build an exporter are logged and downgraded to `None` so a
/// telemetry misconfiguration never prevents the service from starting.
fn select_exporter() -> Option<BoxedSpanExporter> {
    let setting = std::env::var("TELEMETRY_EXPORTER").ok();
    let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();
    let otlp_traces_endpoint = std::env::var("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT").ok();

    let built = match choose_exporter(
        setting.as_deref(),
        otlp_endpoint.as_deref(),
        otlp_traces_endpoint.as_deref(),
    ) {
        ExporterChoice::Disabled => return None,
        ExporterChoice::Xray => env_aws_direct::telemetry::xray_span_exporter(),
        ExporterChoice::OtlpHttp(endpoint) => otel_tracing::otlp_http_exporter(&endpoint),
        ExporterChoice::OtlpGrpc(endpoint) => otel_tracing::otlp_grpc_exporter(&endpoint),
    };

    match built {
        Ok(exporter) => Some(exporter),
        Err(e) => {
            eprintln!("OpenTelemetry exporter init failed ({e}); continuing without OTLP export");
            None
        }
    }
}

/// Initialize tracing/logging for a service, wiring up the configured span
/// exporter (see [`select_exporter`]). Call once at startup.
pub async fn init_tracing(service_name: &str) -> anyhow::Result<()> {
    otel_tracing::init_tracing(service_name, select_exporter())
}

/// Flush and shut down the global tracer provider. Call once at process exit.
pub fn shutdown_tracing() {
    otel_tracing::shutdown_tracing();
}

/// Force-flush buffered spans. In a function-as-a-service runtime, call at the
/// end of each invocation so spans are exported before the execution
/// environment is frozen or torn down.
pub fn force_flush_tracing() {
    otel_tracing::force_flush_tracing();
}

/// Root span for a service entry point, adopting whatever trace context the
/// platform propagated. Re-exported so a binary gets its whole tracing setup
/// from this one module; the span itself is cloud-agnostic, hence its home in
/// `env_utils`.
pub use otel_tracing::entry_span;

#[cfg(test)]
mod tests {
    use super::{choose_exporter, ExporterChoice, DEFAULT_OTLP_HTTP_ENDPOINT};

    /// `choose_exporter` with no OTLP endpoints in the environment.
    fn choose(setting: Option<&str>) -> ExporterChoice {
        choose_exporter(setting, None, None)
    }

    #[test]
    fn xray_and_its_legacy_aliases_all_select_the_same_exporter() {
        // There is one AWS mode; `xray-otlp` and `aws` are names earlier
        // deployments used and must keep resolving.
        for name in ["xray", "xray-otlp", "aws"] {
            assert_eq!(choose(Some(name)), ExporterChoice::Xray);
        }
    }

    #[test]
    fn xray_refuses_to_export_when_an_otlp_endpoint_would_override_it() {
        // opentelemetry-otlp prefers these over the endpoint we hand it, so the
        // spans would miss X-Ray while the requests — signed with SigV4 for the
        // X-Ray host — go to whoever owns the configured one.
        for (endpoint, traces_endpoint) in [
            (Some("https://vendor.example"), None),
            (None, Some("https://vendor.example/v1/traces")),
            (Some("https://a.example"), Some("https://b.example")),
        ] {
            assert_eq!(
                choose_exporter(Some("xray"), endpoint, traces_endpoint),
                ExporterChoice::Disabled,
                "xray with endpoint={endpoint:?} traces_endpoint={traces_endpoint:?} \
                 must not export"
            );
        }
        // Blank is not a redirect, so it must not disable a valid setup.
        assert_eq!(
            choose_exporter(Some("xray"), Some("  "), Some("")),
            ExporterChoice::Xray
        );
    }

    #[test]
    fn the_traces_endpoint_only_constrains_xray() {
        // The otlp-* modes let opentelemetry-otlp read it directly, which is the
        // documented way to point at a vendor's full trace URL.
        assert_eq!(
            choose_exporter(
                Some("otlp-http"),
                None,
                Some("https://vendor.example/v1/traces")
            ),
            ExporterChoice::OtlpHttp(DEFAULT_OTLP_HTTP_ENDPOINT.to_string())
        );
    }

    #[test]
    fn otlp_http_falls_back_to_the_local_collector() {
        assert_eq!(
            choose(Some("otlp-http")),
            ExporterChoice::OtlpHttp(DEFAULT_OTLP_HTTP_ENDPOINT.to_string())
        );
    }

    #[test]
    fn otlp_http_honours_an_explicit_endpoint() {
        assert_eq!(
            choose_exporter(Some("otlp-http"), Some("https://vendor.example"), None),
            ExporterChoice::OtlpHttp("https://vendor.example".to_string())
        );
    }

    #[test]
    fn otlp_grpc_requires_an_endpoint() {
        assert_eq!(
            choose_exporter(Some("otlp-grpc"), Some("http://localhost:4317"), None),
            ExporterChoice::OtlpGrpc("http://localhost:4317".to_string())
        );
        // Unlike http there is no sensible default, so this must not silently
        // export to the wrong place.
        assert_eq!(choose(Some("otlp-grpc")), ExporterChoice::Disabled);
    }

    #[test]
    fn unset_none_and_unknown_values_disable_export() {
        for setting in [None, Some("none"), Some("datadog"), Some("AWS")] {
            assert_eq!(
                choose(setting),
                ExporterChoice::Disabled,
                "setting {setting:?} should disable export"
            );
        }
    }

    #[test]
    fn blank_values_are_treated_as_unset() {
        assert_eq!(choose(Some("   ")), ExporterChoice::Disabled);
        // A blank endpoint must not become the literal endpoint.
        assert_eq!(
            choose_exporter(Some("otlp-http"), Some("  "), None),
            ExporterChoice::OtlpHttp(DEFAULT_OTLP_HTTP_ENDPOINT.to_string())
        );
        assert_eq!(
            choose_exporter(Some("otlp-grpc"), Some("  "), None),
            ExporterChoice::Disabled
        );
    }

    #[test]
    fn surrounding_whitespace_is_tolerated() {
        assert_eq!(choose(Some(" xray ")), ExporterChoice::Xray);
        assert_eq!(
            choose_exporter(Some("otlp-http"), Some(" https://vendor.example "), None),
            ExporterChoice::OtlpHttp("https://vendor.example".to_string())
        );
    }
}
