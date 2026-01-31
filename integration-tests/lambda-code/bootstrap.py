import boto3
import os

dynamodb_endpoint_url = os.environ.get('DYNAMODB_ENDPOINT_URL')
region = os.environ.get('REGION')

def bootstrap_tables():
    client = boto3.client(
        'dynamodb',
        endpoint_url=dynamodb_endpoint_url,
        region_name=region,
        aws_access_key_id='fake',
        aws_secret_access_key='fake'
    )

    events_table_name = "events"
    modules_table_name = "modules"
    policies_table_name = "policies"
    deployments_table_name = "deployments"
    change_records_table_name = "change-records"
    config_table_name = "config"

    # Events table
    client.create_table(
        TableName=events_table_name,
        AttributeDefinitions=[
            {'AttributeName': 'PK', 'AttributeType': 'S'},
            {'AttributeName': 'SK', 'AttributeType': 'S'},
            {'AttributeName': 'PK_base_region', 'AttributeType': 'S'},
        ],
        KeySchema=[
            {'AttributeName': 'PK', 'KeyType': 'HASH'},
            {'AttributeName': 'SK', 'KeyType': 'RANGE'}
        ],
        GlobalSecondaryIndexes=[
            {
                'IndexName': 'RegionIndex',
                'KeySchema': [
                    {'AttributeName': 'PK_base_region', 'KeyType': 'HASH'},
                    {'AttributeName': 'SK', 'KeyType': 'RANGE'}
                ],
                'Projection': {
                    'ProjectionType': 'ALL'
                }
            }
        ],
        BillingMode='PAY_PER_REQUEST',
        Tags=[
            {'Key': 'Name', 'Value': 'EventsTable'}
        ]
    )

    # Modules table
    client.create_table(
        TableName=modules_table_name,
        AttributeDefinitions=[
            {'AttributeName': 'PK', 'AttributeType': 'S'},
            {'AttributeName': 'SK', 'AttributeType': 'S'}
        ],
        KeySchema=[
            {'AttributeName': 'PK', 'KeyType': 'HASH'},
            {'AttributeName': 'SK', 'KeyType': 'RANGE'}
        ],
        BillingMode='PAY_PER_REQUEST',
        Tags=[
            {'Key': 'Name', 'Value': 'ModulesTable'}
        ]
    )

    # Policies table
    client.create_table(
        TableName=policies_table_name,
        AttributeDefinitions=[
            {'AttributeName': 'PK', 'AttributeType': 'S'},
            {'AttributeName': 'SK', 'AttributeType': 'S'}
        ],
        KeySchema=[
            {'AttributeName': 'PK', 'KeyType': 'HASH'},
            {'AttributeName': 'SK', 'KeyType': 'RANGE'}
        ],
        BillingMode='PAY_PER_REQUEST',
        Tags=[
            {'Key': 'Name', 'Value': 'PoliciesTable'}
        ]
    )

    # ChangeRecords table
    client.create_table(
        TableName=change_records_table_name,
        AttributeDefinitions=[
            {'AttributeName': 'PK', 'AttributeType': 'S'},
            {'AttributeName': 'SK', 'AttributeType': 'S'}
        ],
        KeySchema=[
            {'AttributeName': 'PK', 'KeyType': 'HASH'},
            {'AttributeName': 'SK', 'KeyType': 'RANGE'}
        ],
        BillingMode='PAY_PER_REQUEST',
        Tags=[
            {'Key': 'Name', 'Value': 'ChangeRecordsTable'}
        ]
    )

    # Deployments table
    client.create_table(
        TableName=deployments_table_name,
        AttributeDefinitions=[
            {'AttributeName': 'PK', 'AttributeType': 'S'},
            {'AttributeName': 'SK', 'AttributeType': 'S'},
            {'AttributeName': 'deleted_PK', 'AttributeType': 'S'},
            {'AttributeName': 'deleted_PK_base', 'AttributeType': 'S'},
            {'AttributeName': 'module', 'AttributeType': 'S'},
            {'AttributeName': 'module_PK_base', 'AttributeType': 'S'},
            {'AttributeName': 'deleted_SK_base', 'AttributeType': 'S'},
            {'AttributeName': 'next_drift_check_epoch', 'AttributeType': 'N'}
        ],
        KeySchema=[
            {'AttributeName': 'PK', 'KeyType': 'HASH'},
            {'AttributeName': 'SK', 'KeyType': 'RANGE'}
        ],
        GlobalSecondaryIndexes=[
            {
                'IndexName': 'DeletedIndex',
                'KeySchema': [
                    {'AttributeName': 'deleted_PK_base', 'KeyType': 'HASH'},
                    {'AttributeName': 'PK', 'KeyType': 'RANGE'}
                ],
                'Projection': {'ProjectionType': 'ALL'}
            },
            {
                'IndexName': 'ModuleIndex',
                'KeySchema': [
                    {'AttributeName': 'module_PK_base', 'KeyType': 'HASH'},
                    {'AttributeName': 'deleted_PK', 'KeyType': 'RANGE'}
                ],
                'Projection': {'ProjectionType': 'ALL'}
            },
            {
                'IndexName': 'GlobalModuleIndex',
                'KeySchema': [
                    {'AttributeName': 'module', 'KeyType': 'HASH'},
                    {'AttributeName': 'deleted_PK', 'KeyType': 'RANGE'}
                ],
                'Projection': {'ProjectionType': 'ALL'}
            },
            {
                'IndexName': 'DriftCheckIndex',
                'KeySchema': [
                    {'AttributeName': 'deleted_SK_base', 'KeyType': 'HASH'},
                    {'AttributeName': 'next_drift_check_epoch', 'KeyType': 'RANGE'}
                ],
                'Projection': {'ProjectionType': 'ALL'}
            },
            {
                'IndexName': 'ReverseIndex',
                'KeySchema': [
                    {'AttributeName': 'SK', 'KeyType': 'HASH'},
                    {'AttributeName': 'PK', 'KeyType': 'RANGE'}
                ],
                'Projection': {'ProjectionType': 'ALL'}
            }
        ],
        BillingMode='PAY_PER_REQUEST',
        Tags=[
            {'Key': 'Name', 'Value': 'DeploymentsTable'}
        ]
    )

    # Config table
    client.create_table(
        TableName=config_table_name,
        AttributeDefinitions=[
            {'AttributeName': 'PK', 'AttributeType': 'S'},
        ],
        KeySchema=[
            {'AttributeName': 'PK', 'KeyType': 'HASH'},
        ],
        BillingMode='PAY_PER_REQUEST',
        Tags=[
            {'Key': 'Name', 'Value': 'ConfigTable'}
        ]
    )

    client.put_item(
        TableName=config_table_name,
        Item={
            "PK": {
                "S": "all_regions"
            },
            "data": {
                "M": {
                "regions": {
                    "L": [
                    {
                        "S": "us-west-2"
                    }
                    ]
                }
                }
            }
        }
    )

def bootstrap_buckets():
    s3 = boto3.client(
        's3',
        endpoint_url=os.environ.get('MINIO_ENDPOINT'),
        aws_access_key_id=os.environ.get('MINIO_ACCESS_KEY'),
        aws_secret_access_key=os.environ.get('MINIO_SECRET_KEY'),
    )
    s3.create_bucket(Bucket='modules')
    s3.create_bucket(Bucket='policies')
    s3.create_bucket(Bucket='change-records')
    s3.create_bucket(Bucket='providers')
    s3.create_bucket(Bucket='tf-state')
