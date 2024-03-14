from datetime import datetime
import time
import boto3
import json
import os
import random
import string
from boto3.dynamodb.conditions import Key, Attr

region = os.environ.get('REGION')
environment = os.environ.get('ENVIRONMENT')
event_table_name = os.environ.get('DYNAMODB_EVENTS_TABLE_NAME')
module_table_name = os.environ.get('DYNAMODB_MODULES_TABLE_NAME')

dynamodb = boto3.resource('dynamodb')
table = dynamodb.Table(event_table_name)

def handler(event, context):
    # Initialize the CodeBuild client
    codebuild = boto3.client('codebuild')
    sqs = boto3.client('sqs')

    print(event)
    ev = event.get('event')
    module = event.get('module')
    name = event.get('name')
    spec = event.get('spec')
    deployment_id = event.get('deployment_id')
    environment = event.get('environment')

    print(f'deployment_id={deployment_id}')

    # Resolve the module source using the module and environment
    latest_module = get_latest_module(module,  environment)
    if not latest_module:
        print(f'No module found for {module} in {environment}')
        return {
            'statusCode': 400,
            'body': json.dumps(f'No module found for {module} in {environment}')
        }
    
    manifest = latest_module['manifest']
    print(f'manifest={manifest}')

    version = manifest['spec']['version']
    source = manifest['spec']['source']
    source_type = source['type']
    if source_type != 'S3':
        print(f'Source type ({source_type}) is not supported')
        return {
            'statusCode': 400,
            'body': json.dumps(f'Invalid source type ({source_type})')
        }

    bucket = source['bucket']
    path = source['path']

    source_location = f"{bucket}/{path}"

    if deployment_id == '':
        print(f'deployment_id doesn\'t exist')
        new_deployment = False
        # generate deployment_id and verify this is unique, otherwise generate a new one
        exists = True
        while exists:
            print(f'generating new deployment_id')
            deployment_id = f'{module}-{name}-{generate_id(exclude_chars="O0lI", length=3)}'
            exists = check_deployment_exists(deployment_id)
        print(f'new deployment_id={deployment_id}')
    else :
        new_deployment = True
        # look up if deployment_id exists in the table, if it does not, throw an error
        exists = check_deployment_exists(deployment_id)
        if not exists:
            return {
                'statusCode': 400,
                'body': json.dumps(f'Deployment ID {deployment_id} does not exist')
            }
        print(f'deployment_id exists')

    def get_signal_dict(status='TBD', codebuild=False):
        base = {
            'deployment_id': deployment_id,
            'event': ev,
            'module': module,
            'name': name,
            'spec': spec,
        }
        if codebuild:
            placeholder = 'TO_BE_PATCHED_BY_CODEBUILD'
            base.update({
                'id': placeholder,
                'status': placeholder,
                'epoch': placeholder,
                'timestamp': placeholder,
            })
            return base
        else:
            epoch_milliseconds = int(time.time() * 1000)
            base.update({
                'id': f"{deployment_id}-{ev}-{epoch_milliseconds}-{status}",
                'status': status,
                'epoch': epoch_milliseconds,
                'timestamp': datetime.utcnow().replace(microsecond=0).isoformat() + 'Z', # The equivalent to use in bash is "date -u +"%Y-%m-%dT%H:%M:%SZ""
            })
            return base

    row = get_signal_dict(status='received')
    row['metadata'] = {
        'input': event,
    }
    table.put_item(Item=row)
    
    if ev not in ['apply', 'destroy']:
        return {
            'statusCode': 400,
            'body': json.dumps(f'Invalid event type ({ev})')
        }

    project_name = f"infrabridge-worker-{region}-{environment}"

    module_envs = []
    for key, value in spec.items():
        module_envs.append({
            "name": f"TF_VAR_{camel_to_snake(key)}",
            "value": value,
            "type": "PLAINTEXT"
        })

    try:
        # Set up queue for realtime logs
        queue_name = f'logs-{deployment_id}'
        response = sqs.create_queue(QueueName=queue_name)
        # Start the CodeBuild project
        response = codebuild.start_build(
            projectName=project_name,
            environmentVariablesOverride=[
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
                    "value": json.dumps(get_signal_dict(codebuild=True)),
                    "type": "PLAINTEXT"
                }
            ] + module_envs,
            sourceLocationOverride=source_location,
            sourceVersion="",
            sourceTypeOverride=source_type,
            logsConfigOverride={
                'cloudWatchLogs': {
                    'status': 'ENABLED',
                    'groupName': f'/aws/codebuild/{project_name}',
                    'streamName': deployment_id,
                },
            }
        )
        # Log the response from CodeBuild
        print(json.dumps(response, default=str))

        response_dict = {
            'statusCode': 200,
            'body': json.dumps({
                'message': 'Created new deployment successfully' if new_deployment else 'Applied existing deployment successfully',
                'deployment_id': deployment_id
            })
        }
        codebuild_successful = True
    except Exception as e:
        print(e)
        response = str(e)
        # Return an error response
        response_dict = {
            'statusCode': 500,
            'body': json.dumps(
                {
                    'error': str(e),
                }
            )
        }
        codebuild_successful = False

    row = get_signal_dict(status='initiated' if codebuild_successful else 'initation_failed')
    codebuild_id = response['build']['id'] if codebuild_successful else 'NO_ID'
    row['metadata'] = {
        'input': event,
        'codebuild': json.loads(json.dumps(response, default=str))
    }
    row['job_id'] = codebuild_id
    table.put_item(Item=row)

    return response_dict

def camel_to_snake(name):
    import re
    # Insert an underscore before each uppercase letter and convert to lowercase
    return re.sub(r'(?<!^)(?=[A-Z])', '_', name).lower()

def generate_id(exclude_chars="", length=1):
    chars = string.ascii_letters + string.digits
    valid_chars = ''.join(c for c in chars if c not in exclude_chars)
    return ''.join(random.choice(valid_chars) for _ in range(length))

def check_deployment_exists(deployment_id):
    # Query DynamoDB to check if the deployment_id exists
    response = table.query(
        KeyConditionExpression="deployment_id = :deployment_id",
        ExpressionAttributeValues={":deployment_id": deployment_id},
        Limit=1  # We only need to know if at least one item exists
    )

    # If the response contains any items, the deployment_id exists
    return 'Items' in response and len(response['Items']) > 0


def get_latest_module(module, environment):
    print(f'module = {module}, environment = {environment} !')
    entries = get_latest_module_entries(module, environment, 1)
    return entries[0] if entries else None

def get_latest_module_entries(module, environment, num_entries):
    print(f'module_table_name={module_table_name}')
    modules_table = dynamodb.Table(module_table_name)
    response = modules_table.query(
        IndexName='VersionEnvironmentIndex',
        KeyConditionExpression=Key('module').eq(module),
        ScanIndexForward=False,  # False for descending order
        Limit=num_entries,  # Return the latest n entries
        FilterExpression=Attr('environment_version').begins_with(f'{environment}#'),
    )
    print(response)

    if response['Items']:
        return response['Items']
    else:
        return []  # No entries found for the deployment_id