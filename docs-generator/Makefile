
clean:
	rm -rf build

dev-aws: clean
	mkdir -p build
	docker build -t docs-generator-aws -f Dockerfile.dev . --load && \
	docker run -it --rm -v $(PWD)/build:/tmp/build docs-generator-aws

logout-aws:
	docker logout public.ecr.aws

push-aws:
	aws ecr get-login-password --region eu-central-1 | docker login --username AWS --password-stdin 053475148537.dkr.ecr.eu-central-1.amazonaws.com
	docker build -t docs2 -f Dockerfile.aws . --load
	docker tag docs2 053475148537.dkr.ecr.eu-central-1.amazonaws.com/docs-lambda-container-repository:40
	docker push 053475148537.dkr.ecr.eu-central-1.amazonaws.com/docs-lambda-container-repository:40

