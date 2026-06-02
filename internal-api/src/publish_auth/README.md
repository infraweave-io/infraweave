# Publish authorization

Provider-agnostic publish authorization that extracts trusted identity facts
from JWT claims and asks a Rego policy whether the caller may publish a
specific resource (`module`, `stack`, `provider`, `policy`) to a specific
track (`stable`, `rc`, `beta`, `alpha`, `dev`).

It is the only publish authorization path in
[`http_router::ensure_publish_access`](../http_router.rs). If the policy
parameter is unset, identity extraction fails, or the Rego policy denies the
request, publishes are denied.

## Model

The provider is auto-detected from the JWT `iss` claim, which is verified
upstream during JWT validation. GitHub Actions tokens
(`iss == https://token.actions.githubusercontent.com`) route to the
first-class `github_oidc` extractor; every other issuer falls through to the
generic `raw` extractor, where the raw verified claims are exposed for Rego
to interpret. AWS IAM identities are also passed through `raw` facts. Rust
does not choose the authorizing actor; the Rego policy chooses which fact is
authoritative.

For GitHub Actions OIDC, the single provider is `github_oidc`. It exposes
facts that work for both one-repo-per-resource layouts and monorepos:

| Field | Meaning | GitHub OIDC mapping |
|---|---|---|
| `repository` | full repository | `owner/name` |
| `repository_name` | repository without owner | e.g. `tf-module-s3bucket` |
| `repository_owner` | organizational boundary | e.g. `infraweave-io` |
| `ref` | qualifier that gates which tracks may be published | `ref` or `:ref:` segment of `sub` |
| `workflow_ref` | workflow reference, when present | `job_workflow_ref` |
| `workflow_name` | workflow filename stem, when present | e.g. `publish-module-s3bucket` |
| `environment` | GitHub Environment, when present | e.g. `publish-module-s3bucket` |

For other OIDC providers and non-OIDC identities, use `raw`:

| Field | Meaning | Mapping |
|---|---|---|
| `claims` | raw trusted facts object | raw JWT claims, AWS IAM facts, etc. |
| `issuer` | issuer alias | `iss` |
| `subject` | subject alias | `sub` |
| `audience` | audience alias | `aud` |

The Rego policy then:

1. Pins the tenant via `identity.repository_owner == tenant` so a token from
   another org with a matching repo name cannot publish here.
2. Chooses an actor, e.g. `identity.repository_name`,
   `identity.workflow_name`, or `identity.environment`.
3. Looks up the actor prefix for the requested resource type and strips it
   from the actor. The remainder must equal the requested resource name.
4. If the resource type is in `track_exempt_types` (default: `provider` and
   `policy`), allow.
5. Otherwise, allow iff `request.track` is in the trusted or untrusted set,
   selected by `identity.ref == trusted_context`.

Source: [`mod.rs`](mod.rs).

## Configuration

Enable by pointing at the cloud parameter that holds the Rego policy. Without
this the rule is off and all publishes are denied. The provider is detected
automatically from the JWT `iss` claim - no provider env var.

| Variable | Description | Default |
|---|---|---|
| `AUTH_PUBLISH_REGO_POLICY_PARAMETER` | Cloud parameter name containing the raw Rego policy source (AWS SSM Parameter Store, Azure App Configuration, ...) | *(disabled when unset)* |
| `AUTH_PUBLISH_REGO_POLICY_CACHE_TTL_SECONDS` | Warm-process policy cache TTL | `300` |

The actual read is delegated to `env_common::interface::read_config_parameter`,
which dispatches to the active cloud backend - internal-api itself has no
cloud SDK dependency.

All authorization policy choices live in Rego. Do not configure tenant,
actor prefixes, track sets, track exemptions, repository pins, or monorepo
mode through env vars; update the hosted Rego policy instead so there is a
single source of truth. If the policy is not already cached and the parameter
cannot be read, publishes fail closed.

The API passes Rego an input with two top-level fields:

```json
{
  "identity": {
    "provider": "github_oidc",
    "repository": "infraweave-io/modules",
    "repository_name": "modules",
    "repository_owner": "infraweave-io",
    "ref": "refs/heads/main",
    "workflow_ref": "infraweave-io/modules/.github/workflows/publish-module-s3bucket.yml@refs/heads/main",
    "workflow_name": "publish-module-s3bucket",
    "environment": "publish-module-s3bucket"
  },
  "request": {
    "action": "publish",
    "resource_type": "module",
    "resource_name": "s3bucket",
    "track": "stable"
  }
}
```

Default policies should use only `identity` and `request`.

## Policy Examples

The API does not install a bundled default publish policy. It reads the active
policy from `AUTH_PUBLISH_REGO_POLICY_PARAMETER`; without that setting,
publishes are denied.

A common GitHub OIDC policy is intentionally small and covers one convention:
one repo per publishable resource.

| Resource type | Repository prefix |
|---|---|
| `module` | `tf-module-` |
| `stack` | `tf-stack-` |
| `provider` | `tf-provider-` |
| `policy` | `tf-policy-` |

Provider-specific examples live as separate policy files so each one mirrors
a single raw policy you could store in SSM. They are bundled into the Rust
test suite via `include_str!`, so renaming an identity fact in Rust without
updating the example will fail `cargo test`.

Example alternatives:

| Policy | Use case |
|---|---|
| [`github_oidc.rego`](github_oidc.rego) | GitHub Actions OIDC through `github_oidc` |
| [`aws_iam_raw.rego`](aws_iam_raw.rego) | AWS IAM through `raw` |
| [`jwt_user_raw.rego`](jwt_user_raw.rego) | Human/admin JWT users through `raw` |

## Defense In Depth

This rule is the application-layer check. Sitting in front of it should be
an AWS IAM trust policy on the publish-caller role that only lets the right
GitHub workflows assume the role in the first place. IAM grants access to
call the API; this rule decides what the call may do.

For one-repo-per-resource layouts, an IAM trust policy can allow
`repo:infraweave-io/tf-*:*`, and Rego maps `identity.repository_name` to the
resource.

For monorepos, an IAM trust policy should pin the repo, for example
`repo:infraweave-io/modules:*`. Then Rego should also pin
`identity.repository == "infraweave-io/modules"` and use a GitHub-set scoping
fact such as `workflow_name` or `environment`.

## Provider: `github_oidc`

Source: [`github_oidc.rs`](github_oidc.rs).

Maps GitHub Actions OIDC claims onto identity facts:

- `repository`
- `repository_name`
- `repository_owner`
- `ref`
- `workflow_ref`
- `workflow_name`
- `environment`

### One Repo Per Resource

Setup:

```bash
AUTH_PUBLISH_REGO_POLICY_PARAMETER=/infraweave/prod/publish-auth-rego
```

The default Rego policy uses `identity.repository_name` as the actor. A repo
named `infraweave-io/tf-module-s3bucket` can publish `module/s3bucket`; it
cannot publish `module/eks` or `stack/webapp`. A trusted ref
(`refs/heads/main` by default) can publish release tracks, while other refs
can publish only `dev`.

The example pins the GitHub organization with a top-level
`tenant := "infraweave-io"` constant. Replace it with your own organization
before deploying.

### Monorepo

Same configuration - the provider is still auto-detected as `github_oidc`:

```bash
AUTH_PUBLISH_REGO_POLICY_PARAMETER=/infraweave/prod/publish-auth-rego
```

In Rego, use `identity.workflow_name` or `identity.environment` as the actor.
For example, a workflow file named `publish-module-s3bucket.yml` can map to
`module/s3bucket` by stripping the `publish-module-` prefix. Pair workflow
scoping with `CODEOWNERS` on `.github/workflows/`; pair environment scoping
with GitHub Environment required reviewers and branch restrictions.

Always pin the expected monorepo in Rego:

```rego
input.identity.repository == "infraweave-io/modules"
```

## Provider: `raw`

Use `raw` when the caller comes from AWS IAM, a human/admin JWT issuer, or
anything else that is not worth hard-coding in Rust.

Setup:

```bash
AUTH_PUBLISH_REGO_POLICY_PARAMETER=/infraweave/prod/publish-auth-rego
```

The provider is auto-detected as `raw` whenever the JWT `iss` claim is
anything other than the GitHub Actions issuer. AWS IAM publish checks also
use `raw`.

The input shape is still `identity` and `request`, but provider-specific
claims stay under `identity.claims`:

```json
{
  "identity": {
    "provider": "raw",
    "issuer": "https://issuer.example.com",
    "subject": "user-123",
    "claims": {
      "email": "alice@example.com"
    }
  },
  "request": {
    "action": "publish",
    "resource_type": "module",
    "resource_name": "s3bucket",
    "track": "stable"
  }
}
```

The Rego policy should pin the issuer and interpret whatever claims that
issuer guarantees.

For AWS IAM, a full example lives in [`aws_iam_raw.rego`](aws_iam_raw.rego).
It shows how to match either an STS assumed-role ARN normalized to an IAM
role ARN, or the role-id prefix from API Gateway's IAM `userId`.

For human/admin JWT users, [`jwt_user_raw.rego`](jwt_user_raw.rego) shows an
email allow-list based on `identity.email`, with `identity.subject` available
as a fallback when an issuer does not provide stable email claims.

## Adding Another Provider

Start with `raw` for non-GitHub identities. Add a first-class provider
module only when repeated policies would benefit from stable aliases or
careful parsing in Rust. The Rego input shape stays the same; add any
provider-specific facts and rules to the Rego policy.
