REGION ?= eu-north-1

clean:
	rm -rf build

dev-aws: clean
	mkdir -p build
	docker build -t docs-generator-aws -f Dockerfile.dev . --load && \
	docker run -it --rm -v $(PWD)/build:/tmp/build docs-generator-aws

logout-aws:
	docker logout public.ecr.aws

# E.g. make push-aws REGION=us-west-2
push-aws:
	aws ecr get-login-password --region $(REGION) | docker login --username AWS --password-stdin 053475148537.dkr.ecr.$(REGION).amazonaws.com
	docker build -t docs2 -f Dockerfile.aws . --load
	docker tag docs2 053475148537.dkr.ecr.$(REGION).amazonaws.com/docs-lambda-container-repository:47
	docker push 053475148537.dkr.ecr.$(REGION).amazonaws.com/docs-lambda-container-repository:47

