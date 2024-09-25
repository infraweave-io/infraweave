import json
import os
import boto3

dynamodb = boto3.resource('dynamodb')
event_table_name = os.environ.get('DYNAMODB_EVENTS_TABLE_NAME')
table = dynamodb.Table(event_table_name)

def handler(event, context):
    if event.get('type') == 'status':
        return get_status(event)
    elif event.get('type') == 'logs':
        return get_logs(event)
    return json.dumps({
        "statusCode": 400,
        "message":"Invalid event type"
    })

def get_status(event):
    deployment_id = event.get('deployment_id')
    num_entries = event.get('num_entries', 1)
    latest_entry = get_latest_entries(deployment_id, num_entries)
    print(latest_entry)
    return latest_entry

def get_logs(event):
    print("Getting logs...")
    deployment_id = event.get('deployment_id')
    last_job_event = get_latest_event_with_nonempty_job_id(deployment_id)
    print(f"last_job_event: {last_job_event}")
    if last_job_event:
        print(last_job_event)
        # arn:aws:ecs:eu-central-1:123475148473:task/terraform-ecs-cluster/2933fef58cfa421c898b6b462b5a6c74
        job_id = last_job_event.get('job_id').split('/')[-1]
        # Read logs from CloudWatch
        logs_client = boto3.client('logs')
        log_group_name = '/ecs/terraform'
        log_stream_name = f'ecs/terraform-docker/{job_id}'
        response = logs_client.get_log_events(
            logGroupName=log_group_name,
            logStreamName=log_stream_name,
            startFromHead=True
        )

        raw_messages = response.get('events', [])
        messages = [msg['message'] for msg in raw_messages]
        response = "\n".join(messages)
        return json.dumps({
            "statusCode": 200,
            "logs": response,
            "log_group_name": log_group_name,
            "log_stream_name": log_stream_name
        })
    else:
        return json.dumps({
            "statusCode": 404,
            "message":"No logs found for the deployment, did it fail?"
        })


def get_latest_entries(deployment_id, num_entries):
    # Query for the latest entry based on the deployment_id
    response = table.query(
        KeyConditionExpression=boto3.dynamodb.conditions.Key('deployment_id').eq(deployment_id),
        ScanIndexForward=False,  # False for descending order
        Limit=num_entries  # Return the latest n entries
    )

    if response['Items']:
        return response['Items']
    else:
        return []  # No entries found for the deployment_id

def get_all_events(deployment_id):
    response = table.query(
        KeyConditionExpression="deployment_id = :deployment_id",
        ExpressionAttributeValues={
            ":deployment_id": deployment_id
        }
    )
    return response['Items']

def get_latest_event(deployment_id):
    response = table.query(
        KeyConditionExpression="deployment_id = :deployment_id",
        ExpressionAttributeValues={
            ":deployment_id": deployment_id
        },
        ScanIndexForward=False, # Sorts the results in descending order based on the range key
        Limit=1
    )
    return response['Items'][0] if response['Items'] else None

def get_latest_event_with_nonempty_job_id(deployment_id):
    print(f"Querying for deployment_id: {deployment_id}")
    from boto3.dynamodb.conditions import Key, Attr
    response = table.query(
        KeyConditionExpression="deployment_id = :deployment_id",
        # Check if job_id begins with 'arn:aws:ecs'
        FilterExpression=Attr('job_id').begins_with("arn"), # "begins_with(job_id, :job_id)",
        ExpressionAttributeValues={
            ":deployment_id": deployment_id,
            # ":job_id": "arn:aws:ecs",
        },
        ScanIndexForward=False, # Sorts the results in descending order based on the range key
        Limit=10
    )
    print(f"got: {response}")
    return response['Items'][0] if response['Items'] else None

def get_events_by_status(status):
    response = table.query(
        IndexName="StatusIndex",
        KeyConditionExpression="status = :status",
        ExpressionAttributeValues={
            ":status": status
        }
    )
    return response['Items']

# get_logs({"deployment_id":"s3bucket-my-s3bucket-kzE"})