package infraweave.publish

import rego.v1

# GitHub Actions OIDC — one repo per publishable resource.
#
# Convention:
#   tf-module-<name>   -> module/<name>
#   tf-stack-<name>    -> stack/<name>
#   tf-provider-<name> -> provider/<name>
#   tf-policy-<name>   -> policy/<name>
#
# Publishes from the trusted ref (refs/heads/main) may target trusted tracks
# (stable/rc/beta/alpha). Publishes from any other ref may target only `dev`.
# `provider` and `policy` are exempt from track gating.
#
# Defense in depth: pair this with an IAM trust policy that only lets the
# expected GitHub repos assume the publish role. For monorepos or extra
# hardening (pinning `workflow_ref`, `workflow_name`, or `environment`), see
# the README in this directory.
#
# Expected input shape:
#
# {
#   "identity": {
#     "provider": "github_oidc",
#     "repository": "infraweave-io/tf-module-s3bucket",
#     "repository_name": "tf-module-s3bucket",
#     "repository_owner": "infraweave-io",
#     "ref": "refs/heads/main",
#     "workflow_ref": "infraweave-io/tf-module-s3bucket/.github/workflows/publish.yml@refs/heads/main",
#     "workflow_name": "publish",
#     "environment": "publish"
#   },
#   "request": {
#     "action": "publish",
#     "resource_type": "module",
#     "resource_name": "s3bucket",
#     "track": "stable"
#   }
# }

default allow := false

# Replace with your GitHub organization. Pinning the tenant prevents a token
# from another org with a matching repo name from publishing here.
tenant := "infraweave-io"

actor_prefixes := {
	"module": "tf-module-",
	"stack": "tf-stack-",
	"provider": "tf-provider-",
	"policy": "tf-policy-",
}

trusted_context := "refs/heads/main"

trusted_tracks := {"stable", "rc", "beta", "alpha"}

untrusted_tracks := {"dev"}

track_exempt_types := {"provider", "policy"}

allow if {
	input.identity.provider == "github_oidc"
	tenant_matches
	actor_matches_resource
	track_allowed
}

tenant_matches if {
	input.identity.repository_owner == tenant
}

# The "actor" is the identity fact that maps to a publishable resource. Here
# the actor is the repository name; swap for `workflow_name` or `environment`
# in a monorepo. See the README.
actor_matches_resource if {
	actor := input.identity.repository_name
	prefix := actor_prefixes[input.request.resource_type]
	startswith(actor, prefix)
	resource_name := substring(actor, count(prefix), -1)
	resource_name != ""
	resource_name == input.request.resource_name
}

track_allowed if {
	track_exempt_types[input.request.resource_type]
}

track_allowed if {
	track := object.get(input.request, "track", "")
	track != ""
	allowed_tracks[track]
}

allowed_tracks := trusted_tracks if {
	object.get(input.identity, "ref", "") == trusted_context
}

allowed_tracks := untrusted_tracks if {
	object.get(input.identity, "ref", "") != trusted_context
}
