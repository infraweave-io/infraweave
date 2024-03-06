import os
import boto3

dynamodb = boto3.resource('dynamodb')
event_table_name = os.environ.get('DYNAMODB_EVENTS_TABLE_NAME')
table = dynamodb.Table(event_table_name)

def handler(event, context):
    deployment_id = event.get('deployment_id')
    num_entries = event.get('num_entries', 1)
    latest_entry = get_latest_entries(deployment_id, num_entries)
    print(latest_entry)
    return latest_entry


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

def get_events_by_status(status):
    response = table.query(
        IndexName="StatusIndex",
        KeyConditionExpression="status = :status",
        ExpressionAttributeValues={
            ":status": status
        }
    )
    return response['Items']
