
build:
	docker build -t infrabridge-operator:latest -f infrabridge_operator/Dockerfile . --load

test-infrabridge-operator:
	cd ./infrabridge_operator/e2e-tests && ./test_aws_eks.sh

docker_login:
	aws ecr get-login-password --region eu-central-1 | docker login --username AWS --password-stdin 053475148537.dkr.ecr.eu-central-1.amazonaws.com

build_terraform_runner:
	docker build -t terraform_runner:latest -f terraform_runner/Dockerfile . --load

publish_terraform_runner:
	docker tag terraform_runner:latest 053475148537.dkr.ecr.eu-central-1.amazonaws.com/terraform-docker-ecs-test:latest
	docker push 053475148537.dkr.ecr.eu-central-1.amazonaws.com/terraform-docker-ecs-test:latest

try_terraform_runner:
	echo ${AWS_DEFAULT_REGION}
	docker run --rm \
	-e AWS_ACCESS_KEY_ID=${AWS_ACCESS_KEY_ID} \
	-e AWS_SECRET_ACCESS_KEY=${AWS_SECRET_ACCESS_KEY} \
	-e AWS_SESSION_TOKEN=${AWS_SESSION_TOKEN} \
	-e AWS_DEFAULT_REGION=${AWS_DEFAULT_REGION} \
	-e TF_BUCKET=tf-state-eu-central-1-dev-20240909195103349300000003 \
	-e TF_DYNAMODB_TABLE=TerraformStateDynamoDBLocks-eu-central-1-dev \
	-e REGION=eu-central-1 \
	-e ENVIRONMENT=dev \
	-e DEPLOYMENT_ID="" \
	-e TF_JSON_VARS='{"bucket_name":"test-bucket-marius"}' \
	terraform_runner
