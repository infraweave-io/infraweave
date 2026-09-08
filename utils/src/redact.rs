//! Redaction helpers for values that end up in logs or on trace spans.
//!
//! Traces are useful in proportion to how much context they carry, but some of
//! that context is secret — terraform is invoked with credentials inline
//! (`-backend-config=secret_key=…`) and with deployment variables that can hold
//! anything. These helpers keep enough of a value to correlate or eyeball it
//! while dropping the part that matters.

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
}
