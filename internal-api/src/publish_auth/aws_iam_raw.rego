package infraweave.publish

import rego.v1

default allow := false

# Expected input shape:
#
# {
#   "identity": {
#     "provider": "raw",
#     "claims": {
#       "aws_iam_arn": "arn:aws:sts::123456789012:assumed-role/AdminRole/alice@example.com",
#       "aws_iam_user_id": "AROAEXAMPLE123456:alice@example.com"
#     }
#   },
#   "request": {
#     "action": "publish",
#     "resource_type": "module",
#     "resource_name": "s3bucket",
#     "track": "stable"
#   }
# }

admin_aws_role_arns := set()

admin_aws_role_ids := set()

allow if {
	input.identity.provider == "raw"
	admin_aws_role_arns[aws_assumed_role_arn]
}

allow if {
	input.identity.provider == "raw"
	admin_aws_role_ids[aws_assumed_role_id]
}

aws_assumed_role_arn := sprintf("arn:aws:iam::%s:role/%s", [account_id, role_name]) if {
	arn := input.identity.claims.aws_iam_arn
	parts := split(arn, ":")
	parts[2] == "sts"
	account_id := parts[4]
	resource := parts[5]
	resource_parts := split(resource, "/")
	resource_parts[0] == "assumed-role"
	role_name := resource_parts[1]
}

aws_assumed_role_id := split(input.identity.claims.aws_iam_user_id, ":")[0]
