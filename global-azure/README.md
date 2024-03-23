# How to run

* Set up: `terraform apply -auto-approve -var devops_job_pat=fgbtfsjfqczfreblvt5qbsvbzfmkkumhkr5bxrgsqjtjwbpv5x7a`
* Get key: `FUNCKEY=$(az functionapp function keys list --name example-function-appmar --resource-group example-resources --function-name api_infra --query "default" -o tsv)`

* Trigger api_infra: ```
curl "https://example-function-appmar.azurewebsites.net/api/api_infra?code=$FUNCKEY&name=hej" -H "Content-Type: application/json" \
-d '{
    "event": "apply",
    "module": "s3bucket",
    "name": "my-s3-bucket3",
    "environment": "dev",
    "deployment_id": "s3bucket-my-s3-bucket3-5nL",
    "spec": "{ \"bucketName\": \"my-unique-bucket-dep3\", \"region\": \"eu-central-1\" }",
    "annotations": {
        "deploymentId": "s3bucket-my-s3-bucket3-5nL",
        "in-progress": "false",
        "job-id": ""
    }
}'
```

## api_module

* list_latest: ```
curl "https://example-function-appmar.azurewebsites.net/api/api_module?code=$FUNCKEY" -H "Content-Type: application/json" \
-d '{
    "event": "list_latest",
    "environment": "dev"
}'
```

* insert: ```
curl "https://example-function-appmar.azurewebsites.net/api/api_module?code=$FUNCKEY" -H "Content-Type: application/json" \
-d '{
    "event": "insert",
    "environment": "dev"
}'
```