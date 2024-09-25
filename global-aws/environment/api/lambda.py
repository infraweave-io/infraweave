import base64
import boto3
import json
import os

region = os.environ.get('REGION')
environment = os.environ.get('ENVIRONMENT')

tables = {
    'events': os.environ.get('DYNAMODB_EVENTS_TABLE_NAME'),
    'modules': os.environ.get('DYNAMODB_MODULES_TABLE_NAME'),
    'deployments': os.environ.get('DYNAMODB_DEPLOYMENTS_TABLE_NAME')
}

ecs_cluster_name = os.environ.get('ECS_CLUSTER_NAME')
ecs_task_definition = os.environ.get('ECS_TASK_DEFINITION')

module_s3_bucket = os.environ.get('MODULE_S3_BUCKET')

    
def insert_db(event):
    dynamodb = boto3.resource('dynamodb')
    dynamodb_table = tables[event.get('table')]
    dynamodb_row = event.get('data')
    table = dynamodb.Table(dynamodb_table)
    response_dict = table.put_item(Item=dynamodb_row)
    return response_dict

def read_db(event):
    dynamodb = boto3.resource('dynamodb')
    dynamodb_table = tables[event.get('table')]
    print('data:', json.dumps(event.get('data')))
    payload = event.get('data')
    table = dynamodb.Table(dynamodb_table)
    response_dict = table.query(**payload.get('query'))
    return response_dict

def read_logs(event):
    logs = boto3.client('logs')
    payload = event.get('data')
    job_id = payload.get('job_id')
    log_group_name = '/ecs/terraform'
    log_stream_name = f'ecs/terraform-docker/{job_id}'
    response_dict = logs.get_log_events(
        logGroupName=log_group_name,
        logStreamName=log_stream_name,
        startFromHead=True
    )
    return response_dict

def upload_small_file(event):
    s3 = boto3.client('s3')
    payload = event.get('data')
    base64_body = payload.get('base64_content')
    binary_body = base64.b64decode(base64_body)
    s3.put_object(
        Bucket=module_s3_bucket,
        Key=payload.get('key'),
        Body=binary_body
    )

def generate_presigned_url(event):
    s3 = boto3.client('s3')
    payload = event.get('data')
    url = s3.generate_presigned_url(
        ClientMethod='get_object',
        Params={
            'Bucket': module_s3_bucket,
            'Key': payload.get('key')
        },
        ExpiresIn=payload.get('expires_in')
    )
    return {'url': url}

def start_runner(event):
    ecs = boto3.client('ecs')
    payload = event.get('data')
    ecs.run_task(
        cluster=ecs_cluster_name,
        taskDefinition=ecs_task_definition,
        launchType='FARGATE',
        overrides={
            'containerOverrides': [{
                'name': 'terraform-docker',
                'environment': [
                    {
                        "name": "PAYLOAD",
                        "value": json.dumps(payload)
                    }
                ]
            }]
        },
        networkConfiguration={
            'awsvpcConfiguration': {
                'subnets': [os.environ.get('SUBNET_ID')],
                'securityGroups': [os.environ.get('SECURITY_GROUP_ID')],
                'assignPublicIp': 'ENABLED'
            }
        },
        count=1
    )
    return {
            'statusCode': 200,
            'body': 'Runner started'
    }

processes = {
    'insert_db': insert_db,
    'upload_small_file': upload_small_file,
    'read_db': read_db,
    'start_runner': start_runner,
    'read_logs': read_logs,
    'generate_presigned_url': generate_presigned_url
}

def handler(event, context):
    print(event)
    ev = event.get('event')

    if ev not in processes:
        return {
            'statusCode': 400,
            'body': json.dumps(f'Invalid event type ({ev})')
        }
    return processes[ev](event)
