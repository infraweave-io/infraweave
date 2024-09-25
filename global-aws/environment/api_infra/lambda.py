from datetime import datetime
from decimal import Decimal
import time
import boto3
import json
import os
import random
import string
from boto3.dynamodb.conditions import Key, Attr
from boto3.dynamodb.types import TypeSerializer

region = os.environ.get('REGION')
environment = os.environ.get('ENVIRONMENT')
event_table_name = os.environ.get('DYNAMODB_EVENTS_TABLE_NAME')
modules_table_name = os.environ.get('DYNAMODB_MODULES_TABLE_NAME')
module_s3_bucket = os.environ.get('MODULE_S3_BUCKET')
ecs_cluster_name = os.environ.get('ECS_CLUSTER_NAME')  # Set this environment variable
ecs_task_definition = os.environ.get('ECS_TASK_DEFINITION')  # Set this environment variable

dynamodb = boto3.resource('dynamodb')
table = dynamodb.Table(event_table_name)
modules_table = dynamodb.Table(modules_table_name)

ecs = boto3.client('ecs')
# sqs = boto3.client('sqs')

def handler(event, context):
    print(event)
    ev = event.get('event')
    module = event.get('module')
    name = event.get('name')
    deployment_id = event.get('deployment_id')
    environment = event.get('environment')
    module_version = event.get('module_version')
    variables = event.get('variables')

    print(f'deployment_id={deployment_id}')

    if not module_version:
        print(f'No module version provided for {module} in {environment}')
        return {
            'statusCode': 400,
            'body': json.dumps(f'No module version provided for  {module} in {environment}')
        }

    # Resolve the module source using the module and environment
    latest_module = get_module_entry_for_version(module, environment, module_version)
    if not latest_module:
        print(f'No module found for {module} in {environment}')
        return {
            'statusCode': 400,
            'body': json.dumps(f'No module found for {module} in {environment}')
        }
    
    manifest = latest_module['manifest']
    print(f'manifest={manifest}')
    path = latest_module['s3_key']
    source_location = f"{module_s3_bucket}/{path}"

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
    else:
        new_deployment = True
        # look up if deployment_id exists in the table, if it does not, throw an error
        exists = check_deployment_exists(deployment_id)
        if not exists:
            return {
                'statusCode': 400,
                'body': json.dumps(f'Deployment ID {deployment_id} does not exist')
            }
        print(f'deployment_id exists')

    def get_signal_dict(status='TBD'):
        base = {
            'deployment_id': deployment_id,
            'event': ev,
            'module': module,
            'name': name,
            # 'spec': spec,
        }
        epoch_milliseconds = int(time.time() * 1000)
        base.update({
            'id': f"{deployment_id}-{ev}-{epoch_milliseconds}-{status}",
            'status': status,
            'epoch': epoch_milliseconds,
            'timestamp': datetime.utcnow().replace(microsecond=0).isoformat() + 'Z',
        })
        return base

    row = get_signal_dict(status='received')
    row['metadata'] = {
        'input': event,
    }

    # First convert all floats to Decimal in the row
    row = convert_floats_to_decimal(row)
    dynamodb_row = {k: serialize_for_dynamodb(v) for k, v in row.items()}
    table.put_item(Item=dynamodb_row)
    
    if ev not in ['apply', 'destroy']:
        return {
            'statusCode': 400,
            'body': json.dumps(f'Invalid event type ({ev})')
        }

    # module_envs = []
    # for key, value in variables.items():
    #     module_envs.append({
    #         "name": f"TF_VAR_{camel_to_snake(key)}",
    #         "value": value
    #     })

    
    variables_snake_case = {camel_to_snake(key): value for key, value in variables.items()}
    
    print(f'variables={variables}')
    print(f'variables_snake_case={variables_snake_case}')

    try:
        # Set up queue for real-time logs
        queue_name = f'logs-{deployment_id}'
        # response = sqs.create_queue(QueueName=queue_name)

        # Invoke the ECS task
        response = ecs.run_task(
            cluster=ecs_cluster_name,  # Replace with your ECS Cluster name
            taskDefinition=ecs_task_definition,  # Replace with your ECS Task Definition ARN
            launchType='FARGATE',
            overrides={
                'containerOverrides': [{
                    'name': 'terraform-docker',  # Replace with your container name
                    'environment': [
                        {
                            "name": "DEPLOYMENT_ID",
                            "value": deployment_id
                        },
                        {
                            "name": "EVENT",
                            "value": ev
                        },
                        {
                            "name": "MODULE_NAME",
                            "value": module
                        },
                        {
                            "name": "MODULE_VERSION",
                            "value": module_version
                        },
                        {
                            "name": "SIGNAL",
                            "value": json.dumps(get_signal_dict())
                        },
                        {
                            "name": "SOURCE_LOCATION",
                            "value": source_location
                        },
                        {
                            "name": "TF_JSON_VARS",
                            "value": json.dumps(variables_snake_case)
                        }
                    ] # + module_envs
                }]
            },
            networkConfiguration={
                'awsvpcConfiguration': {
                    'subnets': [os.environ.get('SUBNET_ID')],  # Replace with the subnet ID
                    'securityGroups': [os.environ.get('SECURITY_GROUP_ID')],  # Replace with the security group ID
                    'assignPublicIp': 'ENABLED'
                }
            },
            count=1
        )

        # Log the response from ECS
        print(json.dumps(response, default=str))

        if ev == 'destroy':
            message = 'Destroyed deployment successfully'
        elif new_deployment:
            message = 'Created new deployment successfully'
        else:
            message = 'Applied existing deployment successfully'

        response_dict = {
            'statusCode': 200,
            'body': json.dumps({
                'message': message,
                'deployment_id': deployment_id
            })
        }
        ecs_successful = True
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
        ecs_successful = False

    row = get_signal_dict(status='initiated' if ecs_successful else 'initiation_failed')
    ecs_task_arn = response['tasks'][0]['taskArn'] if ecs_successful else 'NO_TASK_ARN'
    row['metadata'] = {
        'input': event,
        'ecs': json.loads(json.dumps(response, default=str))
    }
    row['job_id'] = ecs_task_arn

    # First convert all floats to Decimal in the row
    row = convert_floats_to_decimal(row)
    dynamodb_row = {k: serialize_for_dynamodb(v) for k, v in row.items()}
    table.put_item(Item=dynamodb_row)

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
    # Query for the latest entry based on the deployment_id
    response = modules_table.query(
        KeyConditionExpression='#mod = :module_val',
        FilterExpression='#env = :env_val',
        ExpressionAttributeNames={
            '#mod': 'module',
            '#env': 'environment'
        },
        ExpressionAttributeValues={
            ':module_val': module,
            ':env_val': environment
        },
        ScanIndexForward=False,  # False to sort results by range key in descending order
        Limit=num_entries  # Return the latest n entries
    )

    if response['Items']:
        return response['Items']
    else:
        return []  # No entries found for the deployment_id

def get_module_entry_for_version(module, environment, version):
    # Query for the specific version of the module
    response = modules_table.query(
        IndexName='VersionEnvironmentIndex',  # Use the correct GSI
        KeyConditionExpression='#mod = :module_val AND #ver = :version_val',
        FilterExpression='#env = :env_val',
        ExpressionAttributeNames={
            '#mod': 'module',
            '#ver': 'version',
            '#env': 'environment'
        },
        ExpressionAttributeValues={
            ':module_val': module,
            ':version_val': version,
            ':env_val': environment
        },
        Limit=1  # We only need one result, since we're targeting a specific version
    )

    if response['Items']:
        return response['Items'][0]  # Return the specific item found
    else:
        return None  # No entry found for the module, version, and environment


def convert_floats_to_decimal(item):
    if isinstance(item, list):
        return [convert_floats_to_decimal(i) for i in item]
    elif isinstance(item, dict):
        return {k: convert_floats_to_decimal(v) for k, v in item.items()}
    elif isinstance(item, float):
        return Decimal(str(item))  # Convert float to Decimal
    return item

def serialize_for_dynamodb(item):
    serializer = TypeSerializer()
    if isinstance(item, (dict, list)):  # Only serialize complex types
        return serializer.serialize(item)[next(iter(serializer.serialize(item)))]  # Flatten serialized value
    return item  # Return primitive types as is
