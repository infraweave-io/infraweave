//! Redaction helpers for values that end up in logs or on trace spans.
//!
//! Traces are useful in proportion to how much context they carry, but some of
//! that context is secret — terraform is invoked with credentials inline
//! (`-backend-config=secret_key=…`) and with deployment variables that can hold
//! anything. These helpers keep enough of a value to correlate or eyeball it
//! while dropping the part that matters.

use serde_json::{Map, Value};

/// Characters kept at each end of a masked value.
const KEEP: usize = 3;

/// Values shorter than this are masked completely: revealing three characters
/// from each end of, say, an eight-character secret gives away most of it.
const MIN_LEN_TO_REVEAL: usize = 12;

/// Mask a value, keeping the first and last few characters plus the length.
///
/// The length is included because it is often the useful signal — an empty or
/// truncated credential is visible without exposing the value itself.
pub fn mask_value(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.is_empty() {
        return "(empty)".to_string();
    }
    let len = describe_len(chars.len());
    if chars.len() < MIN_LEN_TO_REVEAL {
        return format!("***({len})");
    }
    let head: String = chars[..KEEP].iter().collect();
    let tail: String = chars[chars.len() - KEEP..].iter().collect();
    format!("{head}***{tail}({len})")
}

fn describe_len(len: usize) -> String {
    match len {
        1 => "1 char".to_string(),
        n => format!("{n} chars"),
    }
}

/// Whether a key name suggests its value is a credential.
///
/// Deliberately does not match a bare `key`: terraform's
/// `-backend-config=key=<state path>` is a useful, non-secret value, while
/// `access_key` and `secret_key` are matched by their more specific prefixes.
pub fn is_sensitive_key(key: &str) -> bool {
    const NEEDLES: &[&str] = &[
        "secret",
        "password",
        "passwd",
        "token",
        "credential",
        "access_key",
        "accesskey",
        "api_key",
        "apikey",
        "private",
        "auth",
    ];
    let lower = key.to_ascii_lowercase();
    NEEDLES.iter().any(|needle| lower.contains(needle))
}

/// Render a command line for logging, masking any argument whose key looks like
/// a credential. Nested forms are handled, so `-backend-config=secret_key=AKIA…`
/// masks only the value.
///
/// Arguments without `=` (subcommands, bare flags) are passed through.
pub fn sanitize_cli_args<I, S>(args: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .map(|arg| {
            let arg = arg.as_ref();
            match split_at_sensitive_key(arg) {
                Some((key, value)) => format!("{key}={}", mask_value(value)),
                None => arg.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Longest free-form text recorded on a span, in characters.
const MAX_SPAN_TEXT: usize = 512;

/// Shorten free-form text for a span attribute, noting what was dropped.
///
/// Terraform hands back the last N lines of its output on failure and nothing
/// bounds how long a line is, so an error carrying a plan fragment or a
/// provider's JSON response can run to tens of kilobytes. That is worth
/// bounding rather than hoping about: the batch processor does not retry, so an
/// export rejected for size loses every span in that batch — for the runner,
/// the batch flushed at shutdown, which is the whole run. The failure it
/// describes is exactly when the trace is worth having.
///
/// Enough is kept to tell one failure from another; the full text is already in
/// the logs, which is where a reader goes next either way.
pub fn truncate_for_span(text: &str) -> String {
    let text = text.trim();
    let mut chars = text.chars();
    // Counting the remainder rather than the whole is what keeps this from
    // walking a huge string twice.
    let kept: String = chars.by_ref().take(MAX_SPAN_TEXT).collect();
    match chars.count() {
        0 => kept,
        dropped => format!("{kept}... (+{dropped} chars, see logs)"),
    }
}

/// Split an argument at the *first* `=` whose left-hand side looks like a
/// credential key, returning that key and everything after it.
///
/// Scanning left to right is what keeps a value containing `=` intact: a
/// right-to-left split of `-backend-config=secret_key=aGVsbG8=` lands on the
/// base64 padding, treats the whole secret as part of the key, and prints it
/// verbatim. Padded base64 is what most credentials look like, so this is the
/// common case rather than an edge one.
///
/// Taking the first match also means a needle anywhere in the prefix masks
/// everything after it, which errs toward hiding too much rather than too
/// little.
fn split_at_sensitive_key(arg: &str) -> Option<(&str, &str)> {
    arg.match_indices('=')
        .map(|(at, _)| (&arg[..at], &arg[at + 1..]))
        .find(|(key, _)| is_sensitive_key(key))
}

/// How much of a redacted value survives.
///
/// What is right differs by destination rather than by the data itself. A log
/// line is read during an incident, where telling two occurrences apart or
/// seeing that a credential arrived empty is the reason to look at all, and it
/// ages out of retention. A value stored on a deployment or rendered back in
/// the TUI has neither that reader nor that end date.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reveal {
    /// Keep the ends and the length, via [`mask_value`]. For logs and spans.
    Ends,
    /// Keep nothing. For values that are persisted or shown to a user.
    Nothing,
}

/// Left in place of a value redacted under [`Reveal::Nothing`], and of any
/// subtree the walk declined to descend.
const REDACTED: &str = "(redacted)";

/// Deepest nesting the walk will follow.
///
/// serde_json stops parsing at 128 levels, so nothing that reached us as text
/// can exceed this and no real document meets the cap. It is here so that a
/// `Value` assembled in code cannot turn redaction into a stack overflow:
/// this runs on the success path, and failing to redact must never be able to
/// take a deployment down with it.
const MAX_DEPTH: usize = 128;

/// Redact the secrets in a JSON document.
///
/// Two signals decide what is secret, because two are available and neither
/// subsumes the other:
///
/// - terraform's own `sensitive` flag, as it appears in `terraform output
///   -json`. Authoritative, since the module author declared it, and the only
///   signal that catches a secret whose name gives nothing away.
/// - the key name, via [`is_sensitive_key`]. A guess, and all there is for
///   deployment variables, which arrive as a plain map with nothing declared.
///
/// Neither is complete — an output the author forgot to mark, or a
/// `connection_string` with a password inside it, still passes through. This
/// removes what announces itself, and a module holding something genuinely
/// secret should not lean on it alone.
///
/// The walk is total: it cannot fail, cannot panic, and returns no error, so
/// callers can redact on the success path without putting a run at risk. Where
/// it cannot make sense of the shape it was handed it redacts more rather than
/// less and keeps going.
pub fn sanitize_json(value: &Value, reveal: Reveal) -> Value {
    walk(value, reveal, 0)
}

fn walk(value: &Value, reveal: Reveal, depth: usize) -> Value {
    if depth >= MAX_DEPTH {
        return Value::String(REDACTED.to_string());
    }
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, child)| (key.clone(), walk_entry(key, child, reveal, depth + 1)))
                .collect(),
        ),
        Value::Array(items) => {
            Value::Array(items.iter().map(|it| walk(it, reveal, depth + 1)).collect())
        }
        // A scalar reaches here only when no key above it matched.
        scalar => scalar.clone(),
    }
}

/// Redact one entry of an object, given what its key and shape say about it.
fn walk_entry(key: &str, child: &Value, reveal: Reveal, depth: usize) -> Value {
    if let Some(map) = child.as_object() {
        if let Some(output) = output_envelope_value(map) {
            let declared_sensitive = map
                .get("sensitive")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if declared_sensitive || is_sensitive_key(key) {
                // Only `value` holds the secret. The envelope around it is what
                // the TUI reads to render the output at all, so replacing the
                // entry wholesale would blank the display rather than redact it.
                let mut redacted = map.clone();
                redacted.insert("value".to_string(), redact(output, reveal, depth + 1));
                return Value::Object(redacted);
            }
        }
    }
    if is_sensitive_key(key) {
        redact(child, reveal, depth)
    } else {
        walk(child, reveal, depth)
    }
}

/// The `value` of a `terraform output -json` entry, if this object is one.
///
/// The shape is `{"value": …, "type": …, "sensitive": bool}`. Requiring
/// `sensitive` alongside `value` is what stops an ordinary variable that
/// happens to have a `value` field from being read as an output envelope.
fn output_envelope_value(map: &Map<String, Value>) -> Option<&Value> {
    if map.contains_key("sensitive") {
        map.get("value")
    } else {
        None
    }
}

/// Redact a value that its key, or the envelope around it, declared secret.
fn redact(value: &Value, reveal: Reveal, depth: usize) -> Value {
    match reveal {
        // Nothing survives, so there is nothing to walk. A container goes whole
        // rather than key by key: the field names inside a credential block can
        // be telling on their own, and no one is debugging from a stored value.
        Reveal::Nothing => Value::String(REDACTED.to_string()),
        Reveal::Ends => mask(value, depth),
    }
}

/// Mask the scalars under a secret value, keeping its structure.
///
/// The shape earns its place in a log — that a credential block has three
/// fields and which of them are empty is the reason to log it — while the
/// leaves are the part that must not survive.
///
/// Keys are not re-checked on the way down: once something above says secret,
/// everything below it is secret whatever it happens to be called.
fn mask(value: &Value, depth: usize) -> Value {
    if depth >= MAX_DEPTH {
        return Value::String(REDACTED.to_string());
    }
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), mask(v, depth + 1)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(|it| mask(it, depth + 1)).collect()),
        Value::String(text) => Value::String(mask_value(text)),
        // Null is left visible: it hides nothing, and an unset credential is a
        // common cause of the failure someone is reading the log to find.
        Value::Null => Value::Null,
        // Numbers and bools go through their rendered form, so a short one
        // disappears entirely like any other short secret.
        scalar => Value::String(mask_value(&scalar.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_values_are_masked_completely() {
        // Revealing 3+3 of an 8-char secret would leave almost nothing hidden.
        assert_eq!(mask_value("hunter2"), "***(7 chars)");
        assert_eq!(mask_value("a"), "***(1 char)");
        assert_eq!(mask_value(""), "(empty)");
    }

    #[test]
    fn long_values_keep_their_ends_and_length() {
        assert_eq!(mask_value("AKIAIOSFODNN7EXAMPLE"), "AKI***PLE(20 chars)");
    }

    #[test]
    fn masking_is_char_safe_not_byte_safe() {
        // Slicing bytes here would panic on a multi-byte boundary.
        let value = "ααααααααααααααα";
        assert_eq!(mask_value(value), "ααα***ααα(15 chars)");
    }

    #[test]
    fn short_text_reaches_the_span_intact() {
        // The common case is a one-line terraform error, which must not be
        // dressed up with a truncation marker it didn't earn.
        assert_eq!(
            truncate_for_span("Error: Invalid provider configuration\n"),
            "Error: Invalid provider configuration"
        );
        assert_eq!(truncate_for_span(""), "");
    }

    #[test]
    fn long_text_is_capped_and_says_how_much_it_dropped() {
        let text = "x".repeat(MAX_SPAN_TEXT + 100);
        let truncated = truncate_for_span(&text);
        assert!(truncated.starts_with(&"x".repeat(MAX_SPAN_TEXT)));
        assert!(
            truncated.ends_with("... (+100 chars, see logs)"),
            "expected a truncation marker, got {truncated}"
        );
        // The point of the cap: what lands on the span stays near the limit
        // however large terraform's output was.
        assert!(truncated.chars().count() < MAX_SPAN_TEXT + 40);
    }

    #[test]
    fn truncation_is_char_safe_not_byte_safe() {
        // Cutting on a byte boundary here would panic mid-character.
        let text = "α".repeat(MAX_SPAN_TEXT + 1);
        assert!(truncate_for_span(&text).ends_with("... (+1 chars, see logs)"));
    }

    #[test]
    fn credential_keys_are_recognised() {
        for key in [
            "-backend-config=secret_key",
            "-backend-config=access_key",
            "AWS_SESSION_TOKEN",
            "my_password",
            "API_KEY",
            "private_key",
        ] {
            assert!(is_sensitive_key(key), "{key} should be sensitive");
        }
    }

    #[test]
    fn state_path_key_is_not_treated_as_a_credential() {
        // `-backend-config=key=<path>` is the state object path, and useful.
        assert!(!is_sensitive_key("-backend-config=key"));
        assert!(!is_sensitive_key("-backend-config=bucket"));
        assert!(!is_sensitive_key("-out"));
    }

    #[test]
    fn sanitize_masks_only_the_credential_arguments() {
        let args = [
            "init",
            "-no-color",
            "-backend-config=bucket=tf-state-prod",
            "-backend-config=key=cli/dev/s3bucket/terraform.tfstate",
            "-backend-config=access_key=AKIAIOSFODNN7EXAMPLE",
            "-backend-config=secret_key=wJalrXUtnFEMIK7MDENGbPxRfiCYEXAMPLEKEY",
        ];
        assert_eq!(
            sanitize_cli_args(args),
            "init -no-color \
             -backend-config=bucket=tf-state-prod \
             -backend-config=key=cli/dev/s3bucket/terraform.tfstate \
             -backend-config=access_key=AKI***PLE(20 chars) \
             -backend-config=secret_key=wJa***KEY(38 chars)"
        );
    }

    #[test]
    fn values_containing_equals_are_still_masked_whole() {
        // Regression: splitting on the last `=` put the base64 padding on the
        // value side, left the secret itself inside the "key", and printed it
        // verbatim onto the span.
        for (arg, secret, expected) in [
            (
                "-backend-config=secret_key=aGVsbG93b3JsZHNlY3JldA==",
                "aGVsbG93b3JsZHNlY3JldA==",
                "-backend-config=secret_key=aGV***A==(24 chars)",
            ),
            (
                "-var=db_password=p@ss=word",
                "p@ss=word",
                "-var=db_password=***(9 chars)",
            ),
            (
                "AWS_SESSION_TOKEN=FwoGZXIvYXdzEBYaDA==",
                "FwoGZXIvYXdzEBYaDA==",
                "AWS_SESSION_TOKEN=Fwo***A==(20 chars)",
            ),
        ] {
            let masked = sanitize_cli_args([arg]);
            assert_eq!(masked, expected);
            assert!(!masked.contains(secret), "{arg} leaked its value: {masked}");
        }
    }

    #[test]
    fn plain_flags_pass_through_untouched() {
        assert_eq!(
            sanitize_cli_args(["plan", "-input=false", "-lock=false", "-out=planfile"]),
            "plan -input=false -lock=false -out=planfile"
        );
    }

    #[test]
    fn deployment_variables_keep_everything_but_the_credentials() {
        // The shape of a claim's variables: the values an operator reads to
        // work out what was deployed sit next to the ones that must not be in
        // the log at all.
        let variables = serde_json::json!({
            "bucket_name": "my-prod-bucket",
            "enable_versioning": true,
            "retention_days": 30,
            "db_password": "correct-horse-battery-staple",
            "api_key": "short",
        });
        assert_eq!(
            sanitize_json(&variables, Reveal::Ends),
            serde_json::json!({
                "bucket_name": "my-prod-bucket",
                "enable_versioning": true,
                "retention_days": 30,
                "db_password": "cor***ple(28 chars)",
                "api_key": "***(5 chars)",
            })
        );
    }

    #[test]
    fn nested_credentials_are_reached() {
        // Regression: masking only the top level left anything a module nested
        // under an object — the common shape for provider config — in cleartext.
        let variables = serde_json::json!({
            "provider": {
                "region": "eu-west-1",
                "credentials": {
                    "access_key_id": "AKIAIOSFODNN7EXAMPLE",
                    "rotate": true,
                    "unused": null,
                },
            },
            "peers": [{"name": "vpc-a", "auth_token": "tok_abcdefghijklmnop"}],
        });
        let masked = sanitize_json(&variables, Reveal::Ends);
        assert_eq!(
            masked,
            serde_json::json!({
                "provider": {
                    "region": "eu-west-1",
                    "credentials": {
                        "access_key_id": "AKI***PLE(20 chars)",
                        // Masked despite its own key looking harmless: the key
                        // above it already said the whole block was secret.
                        "rotate": "***(4 chars)",
                        // Null stays legible — an unset credential is usually
                        // the thing being debugged.
                        "unused": null,
                    },
                },
                "peers": [{"name": "vpc-a", "auth_token": "tok***nop(20 chars)"}],
            })
        );
    }

    #[test]
    fn variables_without_credentials_are_untouched() {
        // Redaction that mangles ordinary variables would push people back to
        // logging the raw value.
        let variables = serde_json::json!({
            "instance_type": "t3.micro",
            "tags": {"owner": "platform", "cost_centre": 4471},
            "subnets": ["subnet-a", "subnet-b"],
        });
        assert_eq!(sanitize_json(&variables, Reveal::Ends), variables);
    }

    #[test]
    fn terraform_outputs_are_redacted_on_their_declared_flag() {
        // The authoritative signal: nothing about the name "connection" says
        // secret, and the name-based rule alone would let it through.
        let output = serde_json::json!({
            "resource_name": {"sensitive": false, "type": "string", "value": "some-name-here"},
            "connection": {"sensitive": true, "type": "string", "value": "postgres://u:p@h/db"},
        });
        assert_eq!(
            sanitize_json(&output, Reveal::Nothing),
            serde_json::json!({
                "resource_name": {"sensitive": false, "type": "string", "value": "some-name-here"},
                // The envelope survives: the TUI reads type and sensitive off it
                // to render the output, so replacing the entry would blank the
                // display rather than redact it.
                "connection": {"sensitive": true, "type": "string", "value": "(redacted)"},
            })
        );
    }

    #[test]
    fn an_output_the_author_forgot_to_mark_is_still_caught_by_name() {
        // The two signals cover for each other: sensitive=false is wrong here,
        // and the name is what saves it.
        let output = serde_json::json!({
            "db_password": {"sensitive": false, "type": "string", "value": "hunter2hunter2"},
        });
        assert_eq!(
            sanitize_json(&output, Reveal::Nothing),
            serde_json::json!({
                "db_password": {"sensitive": false, "type": "string", "value": "(redacted)"},
            })
        );
    }

    #[test]
    fn reveal_decides_how_much_of_a_secret_is_left() {
        let secret = serde_json::json!({"api_key": "AKIAIOSFODNN7EXAMPLE"});
        // Logs keep enough to correlate two occurrences.
        assert_eq!(
            sanitize_json(&secret, Reveal::Ends),
            serde_json::json!({"api_key": "AKI***PLE(20 chars)"})
        );
        // Stored values keep nothing: no one is debugging from them, and they
        // outlive the incident that would have justified it.
        assert_eq!(
            sanitize_json(&secret, Reveal::Nothing),
            serde_json::json!({"api_key": "(redacted)"})
        );
    }

    #[test]
    fn a_value_field_outside_an_output_envelope_is_not_mistaken_for_one() {
        // Without a `sensitive` sibling this is an ordinary variable that
        // happens to have a `value` field, and must walk normally.
        let variables = serde_json::json!({
            "tag": {"value": "prod", "type": "string"},
            "credentials": {"value": "wJalrXUtnFEMIEXAMPLEKEY", "type": "string"},
        });
        assert_eq!(
            sanitize_json(&variables, Reveal::Ends),
            serde_json::json!({
                "tag": {"value": "prod", "type": "string"},
                // Redacted for its own key, not for the envelope rule, so the
                // whole subtree is masked rather than just `value`.
                "credentials": {"value": "wJa***KEY(23 chars)", "type": "***(6 chars)"},
            })
        );
    }

    #[test]
    fn absurd_nesting_is_refused_rather_than_overflowing_the_stack() {
        // Redaction runs on the success path. A pathological document must cost
        // a placeholder, never the deployment.
        let mut deep = serde_json::json!("bottom");
        for _ in 0..(MAX_DEPTH * 4) {
            deep = serde_json::json!({"nest": deep});
        }
        let sanitized = sanitize_json(&deep, Reveal::Ends);
        // Fails closed: the part it would not walk is gone, not passed through.
        assert!(!sanitized.to_string().contains("bottom"));
        assert!(sanitized.to_string().contains("(redacted)"));
    }
}
