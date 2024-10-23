build-operator:
	docker build -t infraweave-operator -f operator/Dockerfile .

test-operator:
	./operator/e2e-tests/test_aws_eks.sh
