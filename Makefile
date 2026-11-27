.PHONY: integration-tests build-images python-sdk-integration-test telemetry-integration-test # Always run, disregard if files have been updated or not

generate-cli-docs:
	@echo "Generating CLI documentation..."
	@PROVIDER=none cargo run --bin cli --quiet -- generate-docs > docs/cli-reference.md
	@echo "CLI documentation generated at docs/cli-reference.md"

build-operator:
	DOCKER_BUILDKIT=1 docker build -t infraweave-operator -f operator/Dockerfile .

build-check:
	@echo "Building with warnings as errors..."
	RUSTFLAGS="-D warnings" cargo build --all-targets

deny:
	cargo deny check

unit-tests: build-check
	cargo test --workspace --exclude integration-tests

integration-tests: aws-integration-tests azure-integration-tests

# Tests own their emulators (floci/floci-az), so they run in parallel via nextest.
JOBS ?= 8
# Provider binaries downloaded by the tests, kept between runs.
DOWNLOAD_CACHE ?= $(CURDIR)/target/download-cache
# Providers downloaded by terraform in the runner tests, kept between runs.
TF_PLUGIN_CACHE ?= $(CURDIR)/target/tf-plugin-cache
# testcontainers and bollard ignore `docker context`; hand them the active one.
ifndef DOCKER_HOST
DOCKER_HOST := $(shell docker context inspect --format '{{ .Endpoints.docker.Host }}' 2>/dev/null)
endif
ifneq ($(DOCKER_HOST),)
export DOCKER_HOST
endif

# For all tests:     make aws-integration-tests
# For specific test: make aws-integration-tests test=stack_tests::test_stack_multiline_policy_with_reference
aws-integration-tests:
	@echo "Running AWS integration tests..."
	@mkdir -p $(DOWNLOAD_CACHE) $(TF_PLUGIN_CACHE)
	PROVIDER=aws \
	INFRAWEAVE_ENV=dev \
	INFRAWEAVE_API_FUNCTION=function \
	AWS_ACCESS_KEY_ID=dummy \
	AWS_SECRET_ACCESS_KEY=dummy \
 	AWS_REGION=us-west-2 \
	TEST_MODE=true \
	CONCURRENCY_LIMIT=1 \
	INFRAWEAVE_DOWNLOAD_CACHE_DIR=$(DOWNLOAD_CACHE) \
	TF_PLUGIN_CACHE_DIR=$(TF_PLUGIN_CACHE) \
	cargo nextest run -p integration-tests -j $(JOBS) $(test) $(if $(test),--no-capture,)

azure-integration-tests:
	@echo "Running Azure integration tests..."
	@mkdir -p $(DOWNLOAD_CACHE) $(TF_PLUGIN_CACHE)
	PROVIDER=azure \
	INFRAWEAVE_ENV=dev \
	INFRAWEAVE_API_FUNCTION=function \
	AZURE_CLIENT_ID=dummy \
	AZURE_CLIENT_SECRET=dummy \
	AZURE_TENANT_ID=dummy \
	REGION=westus2 \
	TEST_MODE=true \
	CONCURRENCY_LIMIT=1 \
	INFRAWEAVE_DOWNLOAD_CACHE_DIR=$(DOWNLOAD_CACHE) \
	TF_PLUGIN_CACHE_DIR=$(TF_PLUGIN_CACHE) \
	cargo nextest run -p integration-tests -j $(JOBS) $(test) $(if $(test),--no-capture,)

# Self-contained: starts a local backend (floci via testcontainers,
# seeded with a provider + module) and runs pytest against the real compiled
# infraweave Python module, building and testing entirely inside Docker.
# Requires only Docker on the host (no host Python/maturin).
python-sdk-integration-test:
	@echo "Running Python SDK integration test (compiled module against local backend)..."
	INFRAWEAVE_RUN_PY_SDK_TEST=1 \
	cargo test -p integration-tests --test python_sdk -- --nocapture --test-threads=1

# Self-contained: starts Jaeger via testcontainers, exports spans through the
# real env_common::telemetry path, and asserts they arrive on the trace context
# that was propagated in. The second suite covers the exit path the deployed
# services rely on: shutdown alone must export the root span.
# Requires only Docker (no cloud credentials).
telemetry-integration-test:
	@echo "Running telemetry integration test (OTLP spans -> Jaeger)..."
	cargo test -p integration-tests --test telemetry -- --nocapture --test-threads=1
	cargo test -p integration-tests --test telemetry_shutdown -- --nocapture --test-threads=1

test: unit-tests integration-tests

clear-docker:
	@echo "Clearing Docker images..."
	@docker stop $$(docker ps -q) && docker rm $$(docker ps -aq) || true
