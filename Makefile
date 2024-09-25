
release-global-aws:
	cd global-aws/environment/api_module && \
      rm -rf package && \
      mkdir package && \
      python3 -m pip install -r requirements.txt --target package --quiet && \
      cp lambda.py package/ && \
      cp schema_module.yaml package/ && \
      cd package && \
      zip -q -r9 ../lambda_function_payload.zip . && \
	  cd .. && \
	  rm -rf package
	cd global-aws && zip -r9 ../global-aws.zip . && cd ..
	aws s3 cp global-aws.zip s3://temporary-tf-release-bucket-jui5345/public/release-0.2.41.zip
