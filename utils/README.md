# Utils

This package contains all small utilities that are common for multiple packages; file-handling, json parsing, logging etc.

## OpenTelemetry tracing

The `otel` feature enables `env_utils::otel_tracing`, which initializes:

- `tracing`/`log` output for local and CloudWatch logs
- optional OpenTelemetry span export
- graceful span flushing via `shutdown_tracing()`

Telemetry export is disabled by default. This keeps the default runtime path at zero extra infrastructure and avoids trying to contact a local collector when no exporter is configured.

`TELEMETRY_EXPORTER` is set by the deployment (Terraform), not baked into the images, so there is a single source of truth per function/task.

> **Do not put a collector inside a Lambda.** It accepts spans over OTLP,
> queues them, and returns — then the execution environment freezes before its
> asynchronous send completes, and they are lost with no error on either side.
> Both the service and the collector report success and nothing arrives. This is
> a property of the freeze, not of any backend: a collector fronting Datadog
> fails the same way, and the usual mitigations are unavailable (the ADOT Lambda
> build has no `decouple` processor and rejects `sending_queue: enabled: false`).
>
> Every mode here exports **in-process**, so `force_flush_tracing()` completes
> before the handler returns. ECS is unaffected — nothing freezes — so a
> collector sidecar remains viable there with a short post-shutdown wait.

There is one AWS mode (`xray`) and two vendor-neutral ones (`otlp-http`, `otlp-grpc`). All export in-process; no image bundles a collector.

### Basic usage

`env_utils::otel_tracing` is the low-level, cloud-agnostic API: it sets up
log formatting and takes an already-built span exporter
(`init_tracing(service_name, exporter)`). Selecting an exporter from the
`TELEMETRY_EXPORTER` environment variable is orchestrated by
`env_common::telemetry`, which is what services normally call:

```rust
use env_common::telemetry;

telemetry::init_tracing("my-service").await?;

// Run application work here.

// Blocking, and off the async worker — see below.
let _ = tokio::task::spawn_blocking(telemetry::shutdown_tracing).await;
```

Cloud-specific exporters live in the matching `env_*_direct` crate
(e.g. AWS X-Ray with SigV4 in `env_aws_direct::telemetry`), so `env_utils`
stays free of any cloud SDK dependency.

Call `shutdown_tracing()` once before process exit so batched spans can be
flushed. It blocks until the batch processor drains, so from an async context
run it on `tokio::task::spawn_blocking` — the processor lives on the same
runtime, and calling it inline deadlocks on a current-thread runtime and starves
a worker on a multi-threaded one.

This matters most for the *root* span, which closes last and so is still queued
at exit. A process that shuts down without a working flush still looks healthy —
the batch processor's periodic flush already shipped every child span — but the
entry-point span is lost, and CloudWatch Application Signals derives a service's
existence from entry-point (`SERVER`-kind) spans, so the service silently never
appears in the Services list.

### Log output

Set `LOG_FORMAT=plain` for human-readable logs in CloudWatch/ECS/Lambda. Plain logs are a minimal one-liner — `<timestamp> <LEVEL> <message>` — with no span context, module target, or ANSI color codes, since that detail is available in the exported traces. Set `LOG_FORMAT=json` for compact JSON logs. If `LOG_FORMAT` is not set, logs are JSON when stderr is not a TTY and pretty/ANSI locally.

Trace export is independent from log formatting, so `LOG_FORMAT=plain` can be used together with any exporter.

```sh
LOG_FORMAT=plain
TELEMETRY_EXPORTER=xray
TELEMETRY_AWS_REGION=eu-west-1
```

JSON logs do not include the full active span stack by default.

Optional log verbosity, both of which apply to **JSON logs only**:

```sh
LOG_INCLUDE_SPANS=true       # include current span and span list on each log event
LOG_SPAN_CLOSE_EVENTS=true   # emit span close timing events
```

`plain` is deliberately fixed at `<timestamp> <LEVEL> <message>` and ignores
both; the pretty local format always emits span close events. Accepted truthy
values are `1`, `true`, `TRUE`, `yes`, and `YES`.

### Naming the deployment

```sh
TELEMETRY_ENVIRONMENT=project1-prod
```

Sets the `deployment.environment` resource attribute. Application Signals builds
a service's identity from name, type and environment, and takes the environment
from here. Unset, it falls back to a per-platform default — `lambda:default`,
`ecs:default` or `generic:default` — which is the same string in every account.

That is survivable across two accounts and useless across a hundred: the console
then lists one row per service per account, all named `reconciler`, all reading
`lambda:default`, separated only by an account column that is not a metric
dimension. Dashboards cannot recover it either, because `SEARCH` labels series
by dimension — a hundred accounts ranked worst-first render as a hundred
identically labelled rows, correctly sorted and unreadable.

The value should identify the deployment rather than the runtime. Leaving it
unset keeps the previous behaviour, so it is additive for anything already
running.

Note that changing it changes the service's identity, so Application Signals
sees a new service. An SLO's `Sli` is mutable and can be repointed, but it is
less work to settle this before creating them.

### Naming the build

```sh
TELEMETRY_SERVICE_VERSION=sha256:19c823be2731dc26   # image digest, tag, or git sha
```

Sets the `service.version` resource attribute, completing the
service/environment/version triple that vendors key deployment tracking on —
`service.name`, `deployment.environment` and this. It is what makes "which build
produced this span" answerable.

It is not derived from `CARGO_PKG_VERSION`, because every crate here uses
`version.workspace = true`: that constant is one number for the whole workspace,
and inside `env_utils` it resolves to this crate rather than to whichever service
is calling. It would name the library, not the deployment. Whatever pinned the
image already knows the digest, so it passes that in.

Note this is a different thing from `faas.version`, which Lambda sets to its own
function version (`$LATEST`) and which says nothing about what you built.

### Associating logs with the service

```sh
TELEMETRY_LOG_GROUP_NAMES=/infraweave/us-west-2/prod/runner   # comma-separated
```

Sets the `aws.log.group.names` resource attribute. The Logs tab of a CloudWatch
Application Signals service page follows that attribute rather than searching
for anything, so without it the tab is empty however healthy the log group is.

On Lambda this defaults to `AWS_LAMBDA_LOG_GROUP_NAME`, which the runtime sets
itself — nothing to configure. On ECS the group is only reachable through the
task metadata endpoint, so the task definition passes it in instead of the
process making an HTTP call at startup.

These are useful while debugging tracing, but they make CloudWatch log lines much noisier.

### AWS X-Ray

The single AWS mode. `xray-otlp` and `aws` are accepted as aliases for
deployments that predate the rename.

```sh
TELEMETRY_EXPORTER=xray
AWS_REGION=eu-west-1
```

The exporter sends OTLP HTTP/protobuf traces directly to:

```text
https://xray.{region}.amazonaws.com/v1/traces
```

Requests are signed with AWS SigV4 using the normal AWS credential provider chain. The region is read from the first available variable:

```sh
TELEMETRY_AWS_REGION
AWS_REGION
AWS_DEFAULT_REGION
```

The runtime role/user needs permission to put traces into X-Ray, for example:

```json
{
  "Effect": "Allow",
  "Action": [
    "xray:PutTraceSegments",
    "xray:PutTelemetryRecords"
  ],
  "Resource": "*"
}
```

If AWS telemetry is configured without a region, tracing initialization logs a warning and continues without span export.

> **Note:** The `https://xray.{region}.amazonaws.com/v1/traces` OTLP endpoint is
> part of X-Ray **Transaction Search**, and the account must have it enabled:
>
> ```sh
> aws xray update-trace-segment-destination --destination CloudWatchLogs
> aws xray get-trace-segment-destination   # must report CloudWatchLogs, not XRay
> ```
>
> Without it every export fails with a 400 — *"The OTLP API is supported with
> CloudWatch Logs as a Trace Segment Destination"* — visible in the service's own
> logs. Note that Lambda functions with `tracing_config { mode = "Active" }` keep
> producing their own segments regardless, named after the function rather than
> the `service.name` set here, so X-Ray can look healthy while none of these
> spans are arriving. If you can't enable Transaction Search (it bills ingested
> spans through CloudWatch Logs), use `otlp-http` against a non-AWS backend
> instead — see below.
>
> Enable it in Terraform with `aws_xray_trace_segment_destination` and
> `aws_xray_indexing_rule` (AWS provider >= 6.62.0). Indexing is the billed
> portion and is what the CloudWatch Traces view queries: at 0% spans are stored
> but never searchable, so that view reads empty.

On Lambda this mode depends on `force_flush_tracing()` running at the end of each
invocation (see `internal-api`'s handler). The execution environment freezes as
soon as the response is returned, so anything left in the batch processor is
otherwise lost. On ECS, `shutdown_tracing()` at process exit does the same job.

Note that ECS does not set `AWS_REGION` the way Lambda does — the runner's task
definition has to provide `AWS_REGION` or `TELEMETRY_AWS_REGION`, or startup logs
a warning and continues without export.

### OTLP HTTP — any vendor or collector

`xray` is one option, not the only one. The `otlp-http` mode is vendor neutral and
speaks standard OTLP/HTTP, so it works with Datadog, Grafana Cloud, Honeycomb,
New Relic, Jaeger, Tempo, or a plain OpenTelemetry Collector — nothing about the
service is AWS-specific.

```sh
TELEMETRY_EXPORTER=otlp-http
# Optional; defaults to http://localhost:4318
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
```

**Against a local agent or collector** — the usual shape for Datadog (the Agent's
OTLP intake) or a sidecar/ADOT collector, which then forwards to the real
backend using its own credentials:

```sh
TELEMETRY_EXPORTER=otlp-http
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
```

**Straight at a hosted OTLP intake**, no collector in between. Vendor API keys go
in `OTEL_EXPORTER_OTLP_HEADERS`, which the exporter reads on its own — there is
no code change or per-vendor support needed:

```sh
TELEMETRY_EXPORTER=otlp-http
OTEL_EXPORTER_OTLP_TRACES_ENDPOINT=https://<vendor-otlp-host>/v1/traces
OTEL_EXPORTER_OTLP_HEADERS=<vendor-api-key-header>=<key>
```

> **Endpoint paths.** `OTEL_EXPORTER_OTLP_ENDPOINT` is treated as a *base* URL and
> gets `/v1/traces` appended; `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` is used exactly
> as given. Use the `_TRACES_` form when your vendor documents a full trace URL,
> otherwise you end up posting to `/v1/traces/v1/traces`.

Note that `OTEL_EXPORTER_OTLP_ENDPOINT` and `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`
take precedence over the endpoint any mode configures internally, including
`xray`. Setting either alongside `TELEMETRY_EXPORTER=xray` is therefore refused:
export is disabled and startup logs why. Spans would not have reached X-Ray in
any case, and the requests would still have been SigV4-signed for the X-Ray host
— handing whoever owns the configured endpoint an `Authorization` header naming
the runtime role's access key. Unset it, or select `otlp-http` to use it
deliberately.

No collector is bundled in any image; every service exports directly. On ECS a
collector sidecar is still workable if you want one, provided the task sets
`TELEMETRY_DRAIN_SECONDS` so the runner waits after `shutdown_tracing()` for the
sidecar to drain before the task tears down:

```sh
TELEMETRY_DRAIN_SECONDS=3   # default 0; only needed with a collector sidecar
```

Without it the task stops the moment the (essential) runner container exits,
killing the sidecar before it forwards the root `terraform_runner` segment — and
X-Ray then drops the whole trace as orphaned subsegments. In-process export needs
no wait, since `shutdown_tracing()` has already drained. On Lambda a sidecar is
not workable at all — see the note at the top.

### OTLP gRPC export

Same vendor-neutral story over gRPC, for a collector, local development, or any
endpoint that accepts OTLP gRPC. `OTEL_EXPORTER_OTLP_HEADERS` applies here too.
Unlike HTTP there is no signal path to append, so the endpoint is used as given.

```sh
TELEMETRY_EXPORTER=otlp-grpc
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317   # required, no default
```

`OTEL_EXPORTER_OTLP_ENDPOINT` is **required** here — unlike `otlp-http` there is
no conventional default port to fall back to, so a missing endpoint logs a
warning and disables export rather than guessing.

### Disable export explicitly

```sh
TELEMETRY_EXPORTER=none
```

Logs still work normally; only span export is disabled.

Anything else — an unset, blank, or unrecognised `TELEMETRY_EXPORTER` — behaves
the same way, logging a warning first when the value was non-empty. Telemetry is
never allowed to stop a service from starting, so every misconfiguration in this
document degrades to logging-only rather than failing startup.

## Redaction

`env_utils::redact` (always available, no feature flag) masks values that would
otherwise reach logs or span attributes. Spans are useful in proportion to the
context they carry, and some of that context is secret: terraform is invoked
with credentials inline (`-backend-config=secret_key=…`) and with deployment
variables that can hold anything.

```rust
use env_utils::redact;

redact::mask_value("AKIAIOSFODNN7EXAMPLE");   // "AKI***PLE(20 chars)"
redact::mask_value("hunter2");                // "***(7 chars)"
redact::mask_value("");                       // "(empty)"

redact::is_sensitive_key("secret_key");       // true
redact::is_sensitive_key("key");              // false — see below

redact::sanitize_cli_args(["terraform", "init", "-backend-config=secret_key=AKIAIOSFODNN7EXAMPLE"]);
// "terraform init -backend-config=secret_key=AKI***PLE(20 chars)"
```

The length is kept because it is often the useful signal — an empty or truncated
credential is visible without exposing the value. Values under 12 characters are
masked entirely, since revealing three characters from each end of a short
secret gives away most of it. Masking is char-based, so multi-byte values are
safe to slice.

`is_sensitive_key` matches substrings (`secret`, `password`, `token`,
`access_key`, `auth`, …) but deliberately **not** a bare `key`: terraform's
`-backend-config=key=<state path>` is a non-secret value worth seeing on a span,
while `access_key` and `secret_key` are caught by their more specific prefixes.

`sanitize_cli_args` splits at the *first* `=` whose left-hand side looks like a
credential key, and masks everything after it; arguments without `=` pass through
untouched. Scanning left to right is what keeps a value containing `=` intact —
splitting from the right lands on base64 padding, mistakes the secret for part of
the key, and prints it verbatim:

```rust
// -backend-config=secret_key=aGVsbG93b3JsZHNlY3JldA==
sanitize_cli_args(args);  // "-backend-config=secret_key=aGV***A==(24 chars)"
```

Taking the first match also means a needle anywhere in the prefix masks
everything after it, erring toward hiding too much rather than too little.
