package infraweave.publish

import rego.v1

default allow := false

# Expected input shape:
#
# {
#   "identity": {
#     "provider": "raw",
#     "issuer": "https://cognito-idp.us-west-2.amazonaws.com/us-west-2_example",
#     "email": "alice@example.com",
#     "subject": "00000000-0000-0000-0000-000000000000",
#     "claims": {
#       "sub": "00000000-0000-0000-0000-000000000000",
#       "email": "alice@example.com",
#       "identities": [
#         {
#           "providerName": "IdentityCenter",
#           "userId": "alice@example.com"
#         }
#       ],
#       "custom:allowed_projects": "123456789012"
#     }
#   },
#   "request": {
#     "action": "publish",
#     "resource_type": "module",
#     "resource_name": "s3bucket",
#     "track": "stable"
#   }
# }

admin_jwt_emails := set()

admin_jwt_usernames := set()

admin_jwt_subjects := set()

allow if {
	input.identity.provider == "raw"
	admin_jwt_emails[lower(input.identity.email)]
}

allow if {
	input.identity.provider == "raw"
	admin_jwt_emails[lower(input.identity.claims.email)]
}

allow if {
	input.identity.provider == "raw"
	admin_jwt_usernames[input.identity.claims["cognito:username"]]
}

allow if {
	input.identity.provider == "raw"
	identity := input.identity.claims.identities[_]
	identity.providerName == "IdentityCenter"
	admin_jwt_emails[lower(identity.userId)]
}

allow if {
	input.identity.provider == "raw"
	admin_jwt_subjects[input.identity.subject]
}
