# Utils

This package contains all small utilities that are common for multiple packages; file-handling, json parsing, logging etc.

## OpenTelemetry tracing

The `otel` feature enables `env_utils::otel_tracing`, which initializes:

- `tracing`/`log` output for local and CloudWatch logs
- optional OpenTelemetry span export
- graceful span flushing via `shutdown_tracing()`

Telemetry export is disabled by default. This keeps the default runtime path at zero extra infrastructure and avoids trying to contact a local collector when no exporter is configured.

### Basic usage

`env_utils::otel_tracing` is the low-level, cloud-agnostic API: it sets up log
formatting and takes an already-built span exporter
(`init_tracing(service_name, exporter)`). It pulls in no cloud SDK — a
cloud-specific exporter belongs in the matching `env_*_direct` crate.

```rust
use env_utils::otel_tracing;

// Logging only.
otel_tracing::init_tracing("my-service", None)?;

// Or exporting spans to a collector.
let exporter = otel_tracing::otlp_http_exporter("http://localhost:4318")?;
otel_tracing::init_tracing("my-service", Some(exporter))?;

// Run application work here.

// Blocking, and off the async worker — see below.
let _ = tokio::task::spawn_blocking(otel_tracing::shutdown_tracing).await;
```

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
