//! Regression test: a process that only calls `shutdown_tracing()` on its way
//! out must still export the spans sitting in the batch queue.
//!
//! This is the terraform runner's exit path, and it was silently broken. The
//! runner closes its root `terraform_runner` span last, then shuts down and
//! exits — but `shutdown_tracing()` only called
//! `global::shutdown_tracer_provider()`, which in opentelemetry 0.27 swaps the
//! global for a no-op and exports nothing. Every child span still arrived,
//! because the batch processor's periodic flush had already shipped them during
//! the multi-second terraform run, so traces looked complete. Only the root
//! span went missing — and with it the runner's entry in CloudWatch Application
//! Signals, which is derived from entry-point (SERVER-kind) spans.
//!
//! Lives in its own test binary because `init_tracing` installs process-global
//! state (tracing subscriber, tracer provider) exactly once, and this test then
//! shuts that provider down.

#[cfg(test)]
mod telemetry_shutdown_tests {
    use env_common::telemetry;
    use integration_tests::scaffold::start_jaeger;
    use serde_json::Value;
    use std::time::Duration;
    use tracing::Instrument;

    const SERVICE_NAME: &str = "telemetry-shutdown-itest";
    const ROOT_SPAN: &str = "runner_root";

    /// Long enough that the batch processor's periodic flush cannot rescue the
    /// span within the test's lifetime. Without this the scheduled flush (5s by
    /// default) would export the root span a moment later and the test would
    /// pass even with a `shutdown_tracing()` that flushes nothing — exactly the
    /// bug being guarded against.
    const NO_PERIODIC_FLUSH_MS: &str = "600000";

    async fn fetch_traces(client: &reqwest::Client, query_url: &str) -> Option<Vec<Value>> {
        let response = client
            .get(format!("{query_url}/api/traces"))
            .query(&[("service", SERVICE_NAME)])
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let body: Value = response.json().await.ok()?;
        let traces = body.get("data")?.as_array()?.clone();
        if traces.is_empty() {
            return None;
        }
        Some(traces)
    }

    async fn wait_for_jaeger(client: &reqwest::Client, query_url: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        loop {
            let up = client
                .get(format!("{query_url}/api/services"))
                .send()
                .await
                .is_ok_and(|response| response.status().is_success());
            if up {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for the jaeger query api"
            );
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    // Multi-threaded for the same reason the runner flushes off the async
    // worker: shutdown blocks until the batch processor drains, and that
    // processor runs on this runtime.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_alone_exports_the_root_span() {
        let (_jaeger, otlp_endpoint, query_url) = start_jaeger().await;
        let client = reqwest::Client::new();
        wait_for_jaeger(&client, &query_url).await;

        std::env::set_var("OTEL_BSP_SCHEDULE_DELAY", NO_PERIODIC_FLUSH_MS);
        std::env::set_var("TELEMETRY_EXPORTER", "otlp-http");
        std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", &otlp_endpoint);
        telemetry::init_tracing(SERVICE_NAME)
            .await
            .expect("tracing should initialize");

        // Mirror terraform_runner's main: a server-kind root span wrapping the
        // work, closed by dropping the instrumented future.
        let root = tracing::info_span!(ROOT_SPAN, otel.kind = "server");
        async {
            tracing::info!("work happening inside the runner");
        }
        .instrument(root)
        .await;

        // The only flush on this path — no force_flush_tracing() beforehand,
        // because the runner has none either.
        tokio::task::spawn_blocking(telemetry::shutdown_tracing)
            .await
            .expect("shutdown should not panic");

        // A short window on purpose: shutdown is supposed to have drained
        // before it returned, so this is only absorbing Jaeger's ingest lag.
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        let traces = loop {
            if let Some(traces) = fetch_traces(&client, &query_url).await {
                break traces;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "shutdown_tracing() returned without exporting anything; \
                 the root span {ROOT_SPAN:?} was dropped with the batch queue"
            );
            tokio::time::sleep(Duration::from_millis(250)).await;
        };

        let names: Vec<String> = traces
            .iter()
            .filter_map(|trace| trace.get("spans").and_then(Value::as_array))
            .flatten()
            .filter_map(|span| {
                span.get("operationName")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect();
        assert!(
            names.iter().any(|name| name == ROOT_SPAN),
            "expected shutdown to export the root span {ROOT_SPAN:?}, got {names:?}"
        );
    }
}
