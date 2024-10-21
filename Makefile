build-operator:
	docker build -t infrabridge-operator -f infrabridge_operator/Dockerfile .

test-operator:
	./infrabridge_operator/e2e-tests/test_aws_eks.sh
