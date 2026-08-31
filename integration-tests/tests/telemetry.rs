//! End-to-end check that configured telemetry actually leaves the process.
//!
//! The unit tests in `env_utils`/`env_common` cover the pieces in isolation —
//! trace id generation, traceparent round-tripping, exporter selection — but
//! they all stop short of the wire. This starts a real trace backend, exports
//! into it over OTLP, and asserts on what arrived, which is the only way to
//! catch a wrong endpoint path, a broken flush, or spans that never build.

#[cfg(test)]
mod telemetry_tests {
    use env_common::telemetry;
    use env_utils::otel_tracing::set_span_parent_from_traceparent;
    use integration_tests::scaffold::start_jaeger;
    use serde_json::Value;
    use std::time::Duration;
    use tracing::Instrument;

    /// A traceparent standing in for one propagated by internal-api / the
    /// reconciler through the runner's `TRACE_ID` container override.
    const UPSTREAM_TRACEPARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    const UPSTREAM_TRACE_ID: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
    const SERVICE_NAME: &str = "telemetry-itest";

    const ROOT_SPAN: &str = "runner_root";
    const CHILD_SPAN: &str = "child_work";

    /// Poll `f` until it yields a value or the deadline passes.
    async fn poll_until<F, Fut, T>(what: &str, timeout: Duration, mut f: F) -> T
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Option<T>>,
    {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Some(value) = f().await {
                return value;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out after {timeout:?} waiting for {what}"
            );
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    /// Fetch the traces Jaeger holds for our service, if the query API is up.
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

    fn span_names(trace: &Value) -> Vec<String> {
        trace
            .get("spans")
            .and_then(Value::as_array)
            .map(|spans| {
                spans
                    .iter()
                    .filter_map(|span| {
                        span.get("operationName")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    // Multi-threaded on purpose: flushing the batch span processor blocks the
    // calling thread while the exporter task drains, which would deadlock on a
    // current-thread runtime.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn exported_spans_reach_the_backend_on_the_callers_trace() {
        let (_jaeger, otlp_endpoint, query_url) = start_jaeger().await;
        let client = reqwest::Client::new();

        poll_until("jaeger query api", Duration::from_secs(60), || async {
            client
                .get(format!("{query_url}/api/services"))
                .send()
                .await
                .ok()
                .filter(|response| response.status().is_success())
                .map(|_| ())
        })
        .await;

        // Configure exactly the way a deployed service would: through the
        // environment, resolved by env_common::telemetry.
        std::env::set_var("TELEMETRY_EXPORTER", "otlp-http");
        std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", &otlp_endpoint);
        telemetry::init_tracing(SERVICE_NAME)
            .await
            .expect("tracing should initialize");

        // Mirror terraform_runner: adopt the propagated trace context as the
        // parent of the root span, then do nested work under it.
        let root = tracing::info_span!(ROOT_SPAN, otel.kind = "server");
        set_span_parent_from_traceparent(&root, UPSTREAM_TRACEPARENT);
        async {
            tracing::info_span!(CHILD_SPAN).in_scope(|| {
                tracing::info!("work happening inside the runner");
            });
        }
        .instrument(root)
        .await;

        // Blocking flush, off the async worker threads.
        tokio::task::spawn_blocking(telemetry::force_flush_tracing)
            .await
            .expect("flush should not panic");

        let traces = poll_until("exported spans", Duration::from_secs(60), || async {
            fetch_traces(&client, &query_url).await
        })
        .await;

        // The whole point of propagation: the spans must land on the caller's
        // trace, not a fresh one the runner invented.
        let trace = traces
            .iter()
            .find(|trace| trace.get("traceID").and_then(Value::as_str) == Some(UPSTREAM_TRACE_ID))
            .unwrap_or_else(|| {
                let seen: Vec<_> = traces
                    .iter()
                    .filter_map(|t| t.get("traceID").and_then(Value::as_str))
                    .collect();
                panic!(
                    "no trace with the propagated id {UPSTREAM_TRACE_ID}; \
                     the runner started its own trace instead. Saw: {seen:?}"
                )
            });

        let names = span_names(trace);
        assert!(
            names.iter().any(|name| name == ROOT_SPAN),
            "expected the root span {ROOT_SPAN:?} to be exported, got {names:?}"
        );
        assert!(
            names.iter().any(|name| name == CHILD_SPAN),
            "expected nested work {CHILD_SPAN:?} on the same trace, got {names:?}"
        );

        // Off the async worker, for the same reason as the flush above.
        tokio::task::spawn_blocking(telemetry::shutdown_tracing)
            .await
            .expect("shutdown should not panic");
    }
}
