.PHONY: integration-tests # Always run, disregard if files have been updated or not

build-operator:
	DOCKER_BUILDKIT=1 docker build -t infraweave-operator -f operator/Dockerfile .

test-operator:
	./operator/e2e-tests/test_aws_eks.sh

unit-tests:
	cargo test -p env_common
	cargo test -p operator
	cargo test -p env_utils

integration-tests:
	PROVIDER=aws INFRAWEAVE_API_FUNCTION=function TEST_MODE=true cargo test -p integration-tests -- --test-threads=1

test: unit-tests integration-tests