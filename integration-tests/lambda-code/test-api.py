import base64
import boto3
import json
import os
from boto3.dynamodb.types import TypeSerializer
import urllib.request
from boto3.s3.transfer import TransferConfig
from botocore.exceptions import ClientError

region = os.environ.get('REGION')
environment = os.environ.get('ENVIRONMENT')
central_account_id = os.environ.get('CENTRAL_ACCOUNT_ID')
dynamodb_arn_prefix = ''#f'arn:aws:dynamodb:{region}:{central_account_id}:table/'

dynamodb_endpoint_url = os.environ.get('DYNAMODB_ENDPOINT_URL')
dynamodb = boto3.resource(
    'dynamodb',
    endpoint_url=dynamodb_endpoint_url,
    region_name=region,
    aws_access_key_id='fake',
    aws_secret_access_key='fake'
)

dynamodb_client = boto3.client(
    'dynamodb',
    endpoint_url=dynamodb_endpoint_url,
    region_name=region,
    aws_access_key_id='fake',
    aws_secret_access_key='fake'
)

tables = {
    'events': os.environ.get('DYNAMODB_EVENTS_TABLE_NAME'),
    'modules': os.environ.get('DYNAMODB_MODULES_TABLE_NAME'),
    'policies': os.environ.get('DYNAMODB_POLICIES_TABLE_NAME'),
    'deployments': os.environ.get('DYNAMODB_DEPLOYMENTS_TABLE_NAME'),
    'change_records': os.environ.get('DYNAMODB_CHANGE_RECORDS_TABLE_NAME'),
    'config': os.environ.get('DYNAMODB_CONFIG_TABLE_NAME'),
}

ecs_cluster_name = os.environ.get('ECS_CLUSTER_NAME')
ecs_task_definition = os.environ.get('ECS_TASK_DEFINITION')

buckets = {
    'modules': os.environ.get('MODULE_S3_BUCKET'),
    'policies': os.environ.get('POLICY_S3_BUCKET'),
    'change_records': os.environ.get('CHANGE_RECORD_S3_BUCKET'),
    'providers': os.environ.get('PROVIDERS_S3_BUCKET'),
}
    
def insert_db(event):
    # dynamodb = boto3.resource('dynamodb')
    dynamodb_table = dynamodb_arn_prefix + tables[event.get('table')]
    dynamodb_table = tables[event.get('table')]
    dynamodb_row = event.get('data')
    table = dynamodb.Table(dynamodb_table)
    response_dict = table.put_item(Item=dynamodb_row, ReturnValues='ALL_OLD')
    return response_dict

def transact_write(event):
    transact_items = []    
    for item in event['items']:
        if 'Put' in item:
            table_name = dynamodb_arn_prefix + tables[item['Put']['TableName']]
            raw_item = item['Put']['Item']
            type_serializer = TypeSerializer()
            serialized_item = {k: type_serializer.serialize(v) for k, v in raw_item.items()}
            transact_items.append({
                'Put': {
                    'TableName': table_name,
                    'Item': serialized_item
                }
            })
        if 'Delete' in item:
            table_name = dynamodb_arn_prefix + tables[item['Delete']['TableName']]
            key = item['Delete']['Key']
            type_serializer = TypeSerializer()
            serialized_key = {k: type_serializer.serialize(v) for k, v in key.items()}

            transact_items.append({
                'Delete': {
                    'TableName': table_name,
                    'Key': serialized_key
                }
            })
    # client = boto3.client('dynamodb')
    response = dynamodb_client.transact_write_items(TransactItems=transact_items)
    return response

def read_db(event):
    # dynamodb = boto3.resource('dynamodb')
    dynamodb_table = dynamodb_arn_prefix + tables[event.get('table')]
    print('data:', json.dumps(event.get('data')))
    payload = event.get('data')
    table = dynamodb.Table(dynamodb_table)
    response_dict = table.query(**payload.get('query'))
    return response_dict

def read_logs(event):
    # logs = boto3.client('logs')
    # payload = event.get('data')
    # job_id = payload.get('job_id')
    # log_group_name = '/ecs/terraform'
    # log_stream_name = f'ecs/terraform-docker/{job_id}'
    # response_dict = logs.get_log_events(
    #     logGroupName=log_group_name,
    #     logStreamName=log_stream_name,
    #     startFromHead=True
    # )
    # return response_dict
    return {
        "status": "success",
        "events": [
            {"message": "Some log message 1"},
            {"message": "Some log message 2"},
            {"message": "Some log message 3"},
        ]
    }

def upload_file_base64(event):
    s3 = boto3.client(
        's3',
        endpoint_url=os.environ.get('MINIO_ENDPOINT'),
        aws_access_key_id=os.environ.get('MINIO_ACCESS_KEY'),
        aws_secret_access_key=os.environ.get('MINIO_SECRET_KEY'),
    )
    payload = event.get('data')
    bucket = buckets[payload.get('bucket_name')]
    base64_body = payload.get('base64_content')
    binary_body = base64.b64decode(base64_body)
    res = s3.put_object(
        Bucket=bucket,
        Key=payload.get('key'),
        Body=binary_body
    )
    return res

def upload_file_url(event):
    s3 = boto3.client(
        's3',
        endpoint_url=os.environ.get('MINIO_ENDPOINT'),
        aws_access_key_id=os.environ.get('MINIO_ACCESS_KEY'),
        aws_secret_access_key=os.environ.get('MINIO_SECRET_KEY'),
    )
    payload = event.get('data')
    bucket = buckets[payload.get('bucket_name')]
    url = payload.get('url')
    key = payload.get('key')

    def object_exists():
        try:
            s3.head_object(Bucket=bucket, Key=key)
        except ClientError as e:
            if e.response['Error']['Code'] == '404':
                return False
            raise
        return True

    if object_exists():
        print(f'Key {key} already exists in S3 bucket {bucket}, skipping upload')
        return {'object_already_exists': True}
    
    CONFIG = TransferConfig(
        multipart_threshold=5 * 1024 * 1024,
        multipart_chunksize=5 * 1024 * 1024,
        max_concurrency=1,
        max_io_queue=1,
        io_chunksize=5 * 1024 * 1024,
    )
    with urllib.request.urlopen(url) as resp:
        # stream-upload the file to S3
        s3.upload_fileobj(
            Fileobj=resp,
            Bucket=bucket,
            Key=key,
            Config=CONFIG
        )
    return {'object_already_exists': False}

def generate_presigned_url(event):
    # s3 = boto3.client('s3', 
    #     region_name=region, endpoint_url=f'https://s3.{region}.amazonaws.com' # https://github.com/boto/boto3/issues/2989
    # )
    s3 = boto3.client(
        's3',
        endpoint_url=os.environ.get('MINIO_ENDPOINT'),
        aws_access_key_id=os.environ.get('MINIO_ACCESS_KEY'),
        aws_secret_access_key=os.environ.get('MINIO_SECRET_KEY'),
    )
    payload = event.get('data')
    url = s3.generate_presigned_url(
        ClientMethod='get_object',
        Params={
            'Bucket': buckets[payload.get('bucket_name')],
            'Key': payload.get('key')
        },
        ExpiresIn=payload.get('expires_in')
    )
    url = 'http://127.0.0.1:' + ':'.join(url.split(':')[2:]) # MinIO is running in a separate network and interfaces with lambda via 172.18.x.x and local rust app using 127.0.0.1
    return {'url': url}

def get_job_status(event):
    payload = event.get('data')
    job_id = payload.get('job_id')
    # For testing purposes, we can simulate different scenarios:
    # - If job_id starts with 'running-', return is_running=True
    # - Otherwise, return is_running=False
    is_running = job_id.startswith('running-') if job_id else False
    return {'job_id': job_id, 'is_running': is_running}

def get_environment_variables(event):
    # Return mock environment variables for testing (matches real lambda.py structure)
    return {
            "DYNAMODB_TF_LOCKS_TABLE_ARN": "arn:aws:dynamodb:us-west-2:123456789012:table/test-tf-locks",
            "TF_STATE_S3_BUCKET": "test-tf-state-bucket",
            "REGION": "us-west-2",
        }

def start_runner(event):
    # ecs = boto3.client('ecs')
    payload = event.get('data')
    
    return {'job_id': 'running-test-job-id'}

processes = {
    'insert_db': insert_db,
    'transact_write': transact_write,
    'upload_file_base64': upload_file_base64,
    'upload_file_url': upload_file_url,
    'read_db': read_db,
    'start_runner': start_runner,
    'read_logs': read_logs,
    'generate_presigned_url': generate_presigned_url,
    'get_job_status': get_job_status,
    'get_environment_variables': get_environment_variables,
    'publish_notification': lambda event: {'status': 'success', 'message': 'Notification published successfully'},
}

from bootstrap import bootstrap_buckets, bootstrap_tables

def handler(event, context):
    if event.get('event') == 'bootstrap_tables':
        return bootstrap_tables()
    if event.get('event') == 'bootstrap_buckets':
        return bootstrap_buckets()
    

    print(event)
    ev = event.get('event')

    if ev not in processes:
        return {
            'statusCode': 400,
            'body': json.dumps(f'Invalid event type ({ev})')
        }
    return processes[ev](event)
