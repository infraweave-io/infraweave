.PHONY: integration-tests # Always run, disregard if files have been updated or not

build-operator:
	DOCKER_BUILDKIT=1 docker build -t infraweave-operator -f operator/Dockerfile .

unit-tests:
	cargo test -p env_common
	cargo test -p operator
	cargo test -p env_utils

integration-tests: aws-integration-tests azure-integration-tests

aws-integration-tests:
	@echo "Running AWS integration tests..."
	PROVIDER=aws \
	INFRAWEAVE_API_FUNCTION=function \
	AWS_ACCESS_KEY_ID=dummy \
	AWS_SECRET_ACCESS_KEY=dummy \
	TEST_MODE=true \
	cargo test -p integration-tests -- --test-threads=1

azure-integration-tests:
	@echo "Running Azure integration tests..."
	PROVIDER=azure \
	INFRAWEAVE_API_FUNCTION=function \
	AZURE_CLIENT_ID=dummy \
	AZURE_CLIENT_SECRET=dummy \
	AZURE_TENANT_ID=dummy \
	REGION=westus2 \
	TEST_MODE=true \
	cargo test -p integration-tests -- --test-threads=1

test: unit-tests integration-tests