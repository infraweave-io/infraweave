# Integration Tests

The integration tests are using all modules combined with real external services (locally) such as Lambda, DynamoDB and S3-compatible storage for AWS, and CosmosDB for Azure.

## 🚀 How to run

Tests are run locally in docker containers. You start the tests for all cloud providers by running:

`make integration-test`

This will take a few minutes.

### 👀 What is happening?

For each test it will:

1. Start the services (lambda, S3, etc) in containers
1. Bootstrap tables and create buckets
1. Run the test
1. Tear down the services

### 🎯 Targeting

You can target a specific cloud provider by running

- AWS: `make aws-integration-tests`
- Azure: `make azure-integration-tests`

## 🔋 What is included?

Tests include:

- Pushing different versioned modules
- Downloading modules
- Setting up a kubernetes cluster

Tests does not yet include:

- Running the terraform_runner (is currently mocked)
