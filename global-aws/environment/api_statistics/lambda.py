from datetime import datetime
import json
import os
import boto3

dynamodb = boto3.resource('dynamodb')
event_table_name = os.environ.get('DYNAMODB_EVENTS_TABLE_NAME')
table = dynamodb.Table(event_table_name)

def handler(event, context):
    deployment_id = event.get('deployment_id')
    num_entries = event.get('num_entries', 1000)
    latest_entries = get_latest_entries(deployment_id, num_entries)
    print(latest_entries)

    # {
    #     'deployment_id': deployment_id,
    #     'event': ev,
    #     'module': module,
    #     'name': name,
    #     'spec': spec,
    #     'status': status,
    #     'timestamp': 'TBD',
    #     'id': f"{deployment_id}-{module}-{name}-{ev}-{unix_timestamp}-{status}"
    # }

    # execution times from started to finished
    
    # Process the data
    deployment_info = {}
    for entry in latest_entries:
        deployment_id = entry['deployment_id']
        if deployment_id not in deployment_info:
            deployment_info[deployment_id] = {'initiated': None, 'started': None, 'finished': None, 'finished_status': False}

        event_time = datetime.fromisoformat(entry['timestamp'].rstrip("Z"))
        deployment_info[deployment_id][entry['event']] = event_time
        if entry['event'] == 'finished':
            deployment_info[deployment_id]['finished_status'] = True

    # Calculate execution times
    for deployment_id, info in deployment_info.items():
        if info['initiated'] and info['started']:
            info['initiation_duration'] = (info['started'] - info['initiated']).total_seconds()
        if info['started'] and info['finished']:
            info['execution_duration'] = (info['finished'] - info['started']).total_seconds()

    return json.dumps(deployment_info, default=json_converter)


def get_latest_entries(deployment_id, num_entries):
    # Query for the latest entry based on the deployment_id
    response = table.query(
        KeyConditionExpression=boto3.dynamodb.conditions.Key('deployment_id').eq(deployment_id),
        ScanIndexForward=False,  # False for descending order
        Limit=1  # Retrieve only the latest entry
    )

    if response['Items']:
        return response['Items'][:num_entries]  # Return the latest n entries
    else:
        return []  # No entries found for the deployment_id


# Function to convert datetime objects to strings
def json_converter(o):
    if isinstance(o, datetime):
        return o.__str__()