.PHONY: integration-tests # Always run, disregard if files have been updated or not

build-operator:
	DOCKER_BUILDKIT=1 docker build -t infraweave-operator -f operator/Dockerfile .

build-check:
	@echo "Building with warnings as errors..."
	RUSTFLAGS="-D warnings" cargo build --all-targets

unit-tests: build-check
	cargo test --workspace --exclude integration-tests

integration-tests: aws-integration-tests azure-integration-tests

aws-integration-tests:
	@echo "Running AWS integration tests..."
	PROVIDER=aws \
	INFRAWEAVE_ENV=dev \
	INFRAWEAVE_API_FUNCTION=function \
	AWS_ACCESS_KEY_ID=dummy \
	AWS_SECRET_ACCESS_KEY=dummy \
 	AWS_REGION=us-east-1 \
	TEST_MODE=true \
	CONCURRENCY_LIMIT=1 \
	cargo test -p integration-tests -- --test-threads=1

azure-integration-tests:
	@echo "Running Azure integration tests..."
	PROVIDER=azure \
	INFRAWEAVE_ENV=dev \
	INFRAWEAVE_API_FUNCTION=function \
	AZURE_CLIENT_ID=dummy \
	AZURE_CLIENT_SECRET=dummy \
	AZURE_TENANT_ID=dummy \
	REGION=westus2 \
	TEST_MODE=true \
	CONCURRENCY_LIMIT=1 \
	cargo test -p integration-tests -- --test-threads=1

test: unit-tests integration-tests

clear-docker:
	@echo "Clearing Docker images..."
	@docker stop $$(docker ps -q) && docker rm $$(docker ps -aq) || true