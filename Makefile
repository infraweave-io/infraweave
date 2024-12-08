.PHONY: integration-tests # Always run, disregard if files have been updated or not

build-operator:
	DOCKER_BUILDKIT=1 docker build -t infraweave-operator -f operator/Dockerfile .

test-operator:
	./operator/e2e-tests/test_aws_eks.sh

integration-tests:
	INFRAWEAVE_API_FUNCTION=function TEST_MODE=true cargo test -p integration-tests -- --test-threads=1
