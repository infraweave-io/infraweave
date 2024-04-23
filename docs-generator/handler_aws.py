import boto3
import os
from source.generator import run_and_zip
from source.input import ModuleDef

DOCS_BUCKET = os.environ['DOCS_BUCKET']

dynamodb = boto3.resource('dynamodb')
modules_table_name = os.environ.get('DYNAMODB_MODULES_TABLE_NAME')
modules_table = dynamodb.Table(modules_table_name)

def upload_to_s3(file_name, bucket, object_name=None):
    if object_name is None:
        object_name = file_name

    s3_client = boto3.client('s3')
    response = s3_client.upload_file(file_name, bucket, object_name)
    return response

# def get_latest_entries(module, environment, num_entries):
#     # Query for the latest entry based on the deployment_id
#     response = modules_table.query(
#         KeyConditionExpression='#mod = :module_val',
#         FilterExpression='#env = :env_val',
#         ExpressionAttributeNames={
#             '#mod': 'module',
#             '#env': 'environment'
#         },
#         ExpressionAttributeValues={
#             ':module_val': module,
#             ':env_val': environment
#         },
#         ScanIndexForward=False,  # False to sort results by range key in descending order
#         Limit=num_entries  # Return the latest n entries
#     )

#     if response['Items']:
#         return response['Items']
#     else:
#         return []  # No entries found for the deployment_id

def get_all_modules():
    response = modules_table.scan()
    dynamodb_items = response['Items']
    items = dynamodb_items

    modules = []
    for item in items:
        module = ModuleDef(
            module_name=item['module_name'],
            description=item['description'],
            version=item['version'],
            timestamp=item['timestamp'],
            tf_outputs=item['tf_outputs'],
            tf_variables=item['tf_variables']
        )
        modules.append(module)
    return modules

def get_all_modules_dict():
    modules = get_all_modules()
    module_dict = {}
    for module in modules:
        if module.module_name not in module_dict:
            module_dict[module.module_name] = []
        module_dict[module.module_name].append(module)
    return module_dict

def handler(event, context):
    print("Running docs generator...")

    # modules = get_latest_entries('s3bucket', 'dev', 1)
    module_dict = get_all_modules_dict()
    print(f"Found {len(module_dict)} modules")
    print(module_dict)

    zip_path = '/tmp/webpage_archive'
    run_and_zip(zip_path, module_dict)
    
    available_at = f'NO_STORE_LOCATION_IS_SET'

    if os.getenv('DOCS_BUCKET'):
        upload_to_s3(f'{zip_path}.zip', DOCS_BUCKET, 'docs/latest.zip')
        available_at = f's3://{DOCS_BUCKET}/docs/latest.zip'
        message = f'Finished generating new docs! Available at: {available_at}'
    else:
        return {
            'statusCode': 500,
            'body': 'DOCS_BUCKET environment variable is not set!',
        }

    print(message)
    return {
        'statusCode': 200,
        'body': message,
    }
handler(None, None)