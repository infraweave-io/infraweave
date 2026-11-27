# Chat eval suite

Deterministic scenarios for the agent loop. No AWS, no network, no Docker —
`cargo test -p infraweave-chat` is the whole story.

One caveat: `reqwest` honours `HTTP_PROXY` / `ALL_PROXY` and does not exempt
loopback on its own. If your shell sets a proxy, also set
`NO_PROXY=127.0.0.1,localhost`, or the stub traffic goes to the proxy and the
scenarios fail. CI sets this on the unit-test job.

## What the fakes do

- `support::fake_llm::FakeLlm` replays a scripted `Vec<LlmResponse>`, one per
  `converse` call, and records the `(system, messages, tool_names)` it received.
- `support::stub_api::StubApi` serves canned JSON from a real axum server on an
  ephemeral port, so `ApiClient` runs its true HTTP and deserialization path.
  Routes match the exact path and query string, with no path-only fallback. An
  unregistered path returns 404 and is recorded in `unmatched()`. The 404 alone
  is not enough: tools that read through `get_optional` (e.g. `debug_deployment`)
  or discard errors (its events fetch) turn it into an ordinary result.
- `support::scenario::Scenario` wires both to the real `agent::run` and the real
  tool registry, then hands back the `AgentEvent` stream.
  It panics if the agent emitted an `Error` event, left scripted responses
  unused, or requested a path with no fixture, since each means the run diverged
  from the scenario. Call `.allow_unused_script()` when stopping early is the
  point (the iteration cap), and `.allow_unmatched_requests()` when a missing
  fixture is (a tool failing on a 404).
- `support::fixtures` holds internal-api payloads that deserialize into the real
  `env_defs` types. A fixture missing a required field makes the tool fail, so a
  success-path scenario quietly becomes an error-path one. Build on these, and
  assert `tool_errors().is_empty()` whenever a scenario expects success.

## Adding a scenario

1. Write the model script: one `LlmResponse` per turn you expect.
2. Register a stub route for every API path the tools will touch. The runner
   names any path you missed.
3. Assert on `tool_calls_made()`, `tool_input()`, `requested_paths`,
   `tool_result_blocks()` or `history()`.

```rust
let result = Scenario::new("why did payments-api fail?")
    .with_session_defaults(PROJECT_ID, "us-west-2", "prod", "stable")
    .with_api_route(PROJECTS, one_project(&["us-west-2"]))
    .with_api_route("/api/v1/deployment/123456789012/us-west-2/prod/payments-api", dep())
    .with_api_route("/api/v1/events/123456789012/us-west-2/prod/payments-api", json!([]))
    .with_script(vec![debug_call(), text_response("...")])
    .run()
    .await;

assert_eq!(result.tool_calls_made(), vec!["debug_deployment"]);
assert!(result.tool_errors().is_empty());
```

## What this suite does not test

Model judgement. The script decides which tools get called, so nothing here tells
you whether Claude would have picked the right tool for an ambiguous question.
That needs a live suite against real Bedrock — deliberately out of scope.

Cache-point placement sits below `FakeLlm`, in the Bedrock mapping, so this suite
asserts only its prerequisites: a byte-identical system prompt and a stable tool
order across iterations. The tool-list cache point itself is pinned by a unit test
in `src/llm/bedrock.rs` (`tool_config_ends_with_a_cache_point_after_every_tool`).
The system-prompt cache point is still unasserted, because it is built inline in
`converse` and `converse_stream`. Extracting it into a shared helper would make it
testable too.

## Rules

- **Every new tool ships with a scenario in the same commit.** A tool without one
  is not done.
- Keep fixtures minimal — the smallest JSON that deserializes and exercises the path.
- Assert on behaviour, not on prose. Never assert the exact wording of a
  markdown tool output; assert that the identifiers and statuses appear in it.
