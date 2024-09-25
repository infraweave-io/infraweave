import json
import os
from decimal import Decimal
import boto3

dynamodb = boto3.resource('dynamodb')
event_table_name = os.environ.get('DYNAMODB_DEPLOYMENTS_TABLE_NAME')
table = dynamodb.Table(event_table_name)

def handler(event, context):
    deployment_id = event.get('deployment_id', None)
    if deployment_id is None:
        res = get_all_deployments()
    elif type(deployment_id) is str:
        res = get_deployment_id(deployment_id)
    elif event.get('type') == 'set_deployment':
        res = set_deployment(event)
    return json.dumps(res, cls=DecimalEncoder)

def get_all_deployments():
    response = table.query(
        IndexName='DeletedIndex',
        KeyConditionExpression='deleted = :deleted',
        ExpressionAttributeValues={':deleted': 0}
    )
    return response['Items']

def get_deployment_id(deployment_id):
    response = table.query(
        IndexName="DeploymentIdIndex",
        KeyConditionExpression="deployment_id = :deployment_id",
        ExpressionAttributeValues={
            ":deployment_id": deployment_id
        }
    )
    return response['Items']

def set_deployment(event):
    data = event.get('data')
    table.put_item(Item=event)

class DecimalEncoder(json.JSONEncoder):
    def default(self, obj):
        if isinstance(obj, Decimal):
            return float(obj)  # Convert decimal instances to floats
        return super(DecimalEncoder, self).default(obj)
