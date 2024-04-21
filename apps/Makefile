
build:
	docker build -t infrabridge-operator:latest -f infrabridge_operator/Dockerfile . --load

test-infrabridge-operator:
	cd ./infrabridge_operator/e2e-tests && ./test_aws_eks.sh
