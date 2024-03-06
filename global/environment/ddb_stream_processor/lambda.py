import os
import boto3
import json

dynamodb = boto3.resource('dynamodb')

def handler(event, context):
    sns = boto3.client('sns')
    topic_arn = os.environ['SNS_TOPIC_ARN']
    region = os.environ['REGION']

    import json
    print(json.dumps(event))
    
    for record in event['Records']:
        if record['eventName'] == 'INSERT':  # Adjust based on your needs
            # Extract the desired data from the record's dynamodb item
            dynamodb_data = record['dynamodb']['NewImage']
            # Construct a message (customize as needed)
            message = {
                "deployment_id": dynamodb_data['deployment_id']['S'], # Make a request to the statusApi using this deployment_id to not have to get these changes
                "epoch": dynamodb_data['epoch']['N'],
                "status": dynamodb_data['status']['S'],
                "module": dynamodb_data['module']['S'],
                "name": dynamodb_data['name']['S'],
                # "spec": dynamodb_data['spec']['S'],
                "event": dynamodb_data['event']['S'],
                # "timestamp": dynamodb_data['timestamp']['S'],
                # "metadata": dynamodb_data['metadata']['M']
            }
            # Publish to SNS
            print('Publishing to SNS')
            sns.publish(
                TopicArn=topic_arn,
                Message=json.dumps(message)
            )
    return {
        'statusCode': 200,
        'body': json.dumps('Successfully processed DynamoDB record.')
    }
