import boto3
import json

def handler(event, context):
    # Initialize the CodeBuild client
    codebuild = boto3.client('codebuild')
    print(event)
    deployment_id = event.get('deployment_id')
    ev = event.get('event')

    if ev not in ['apply', 'destroy']:
        return {
            'statusCode': 400,
            'body': json.dumps(f'Invalid event type ({ev})')
        }


    project_name = "terraform-s3bucket-eu-central-1-dev"

    spec = event.get('spec')

    module_envs = []
    for key, value in spec.items():
        module_envs.append({
            "name": f"TF_VAR_{camel_to_snake(key)}",
            "value": value,
            "type": "PLAINTEXT"
        })
    
    try:
        # Start the CodeBuild project
        response = codebuild.start_build(
            projectName=project_name,
            environmentVariablesOverride=[
                {
                    "name": "ID",
                    "value": "s3bucket-marius-123",
                    "type": "PLAINTEXT"
                },
                # {
                #     "name": "INPUT_VARIABLES_JSON",
                #     "value": "{\"bucket_name\": \"my-bucket2-test-1232432fdsf\"}",
                #     "type": "PLAINTEXT"
                # },
                {
                    "name": "DEPLOYMENT_ID",
                    "value": deployment_id,
                    "type": "PLAINTEXT"
                },
                {
                    "name": "EVENT",
                    "value": ev,
                    "type": "PLAINTEXT"
                },
            ] + module_envs,
            sourceLocationOverride="tf-modules-bucket-482njk4krnw/s3bucket/release-0.1.0.zip",
            sourceVersion="",
            sourceTypeOverride="S3",
        )
        
        # Log the response from CodeBuild
        print(json.dumps(response, default=str))
        
        # Return a successful response
        return {
            'statusCode': 200,
            'body': json.dumps('CodeBuild project started successfully.')
        }
    except Exception as e:
        print(e)
        # Return an error response
        return {
            'statusCode': 500,
            'body': json.dumps('Error starting CodeBuild project.')
        }

def camel_to_snake(name):
    import re
    # Insert an underscore before each uppercase letter and convert to lowercase
    return re.sub(r'(?<!^)(?=[A-Z])', '_', name).lower()
