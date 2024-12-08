# Integration tests

The integration tests consists of all packages and are used the same interfacing functions as cli, Python SDK and Kubernetes operator together with real external services such as Lambda, DynamoDB and S3-compatible storage.

This encompasses how it works when deployed. The services are run without authentication locally in docker containers, and are set up + managed automatically when running "make integration-test".
