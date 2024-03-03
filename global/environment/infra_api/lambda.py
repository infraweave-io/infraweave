from datetime import datetime
import time
import boto3
import json
import os

def handler(event, context):
    # Initialize the CodeBuild client
    codebuild = boto3.client('codebuild')
    dynamodb = boto3.resource('dynamodb')

    print(event)
    deployment_id = event.get('deployment_id')
    ev = event.get('event')
    module = event.get('module')
    name = event.get('name')
    spec = event.get('spec')

    if ev not in ['apply', 'destroy']:
        return {
            'statusCode': 400,
            'body': json.dumps(f'Invalid event type ({ev})')
        }

    region = os.environ.get('REGION')
    environment = os.environ.get('ENVIRONMENT')
    project_name = f"terraform-{module}-{region}-{environment}"

    event_table_name = os.environ.get('DYNAMODB_EVENTS_TABLE_NAME')

    spec = event.get('spec')

    module_envs = []
    for key, value in spec.items():
        module_envs.append({
            "name": f"TF_VAR_{camel_to_snake(key)}",
            "value": value,
            "type": "PLAINTEXT"
        })
    
    def get_signal_dict(status):
        unix_timestamp = int(time.time())
        return {
            'deployment_id': deployment_id,
            'event': ev,
            'module': module,
            'name': name,
            'spec': spec,
            'status': status,
            'timestamp': 'TBD',
            'id': f"{deployment_id}-{module}-{name}-{ev}-{unix_timestamp}-{status}"
        }

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
                {
                    "name": "SIGNAL",
                    "value": json.dumps(get_signal_dict('TBD')),
                    "type": "PLAINTEXT"
                }
            ] + module_envs,
            sourceLocationOverride="tf-modules-bucket-482njk4krnw/s3bucket/release-0.1.0.zip",
            sourceVersion="",
            sourceTypeOverride="S3",
        )
        # Log the response from CodeBuild
        print(json.dumps(response, default=str))

        response_dict = {
            'statusCode': 200,
            'body': json.dumps('CodeBuild project started successfully.')
        }
    except Exception as e:
        print(e)
        # Return an error response
        response_dict = {
            'statusCode': 500,
            'body': json.dumps('Error starting CodeBuild project.')
        }

    table = dynamodb.Table(event_table_name)
    row = get_signal_dict('initiated')
    row['metadata'] = {
        'input': event,
        'codebuild': json.loads(json.dumps(response, default=str))
    }
    # The equivalent of below in bash is "date -u +"%Y-%m-%dT%H:%M:%SZ""
    row['timestamp'] = datetime.utcnow().replace(microsecond=0).isoformat() + 'Z'
    table.put_item(Item=row)

    return response_dict

def camel_to_snake(name):
    import re
    # Insert an underscore before each uppercase letter and convert to lowercase
    return re.sub(r'(?<!^)(?=[A-Z])', '_', name).lower()
