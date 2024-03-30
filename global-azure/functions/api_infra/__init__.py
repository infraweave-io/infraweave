import base64
import json
import os
import random
import logging
import string
import requests
import azure.functions as func
from azure.data.tables import TableServiceClient
from azure.core.exceptions import ResourceNotFoundError

region = os.environ.get('REGION')
environment = os.environ.get('ENVIRONMENT')
event_table_name = os.environ.get('STORAGE_TABLE_EVENTS_TABLE_NAME')
module_table_name = os.environ.get('STORAGE_TABLE_MODULES_TABLE_NAME')
module_connection_string = os.getenv("AZURE_MODULES_TABLE_CONN_STR")

# Modules Table
module_table_name = os.environ.get('STORAGE_TABLE_MODULES_TABLE_NAME')
module_connection_string = os.getenv("AZURE_MODULES_TABLE_CONN_STR")
module_table_service = TableServiceClient.from_connection_string(conn_str=module_connection_string)
modules_table_client = module_table_service.get_table_client(table_name=module_table_name)

def main(req: func.HttpRequest) -> func.HttpResponse:
    organization = os.getenv('organization')
    project = os.getenv('project')
    pipeline_id = os.getenv('pipeline_id')
    pat = os.getenv('pat')

    tf_storage_account = os.getenv('tf_storage_account')
    tf_container = os.getenv('tf_storage_container')
    tf_storage_access_key = os.getenv('tf_storage_access_key')
    deployment_id = os.getenv('deployment_id')
    environment = os.getenv('environment')
    region = os.getenv('region')
    tf_dynamodb_table = os.getenv('tf_dynamodb_table')

    try:
        # Attempt to get JSON body
        event = req.get_json()
    except ValueError:
        # If there is no JSON body, or if JSON is invalid, set req_body to None or handle as needed
        return func.HttpResponse(
            "Please pass JSON in the request body",
            status_code=400
        )

    logging.info(event)
    ev = event.get('event')
    module = event.get('module')
    name = event.get('name')
    spec = event.get('spec') # TODO: align AWS for this instead of json.loads(event.get('spec', "{}"))
    deployment_id = event.get('deployment_id')
    environment = event.get('environment')
    print(f'deployment_id={deployment_id}')

    # Resolve the module source using the module and environment
    try:
        latest_module = get_latest_module(module,  environment)
        logging.info(f'latest_module={latest_module}')
    except Exception as e:
        return func.HttpResponse(
            f'Error occurred while fetching module: {e}',
            status_code=400
        )
    if not latest_module:
        print(f'No module found for {module} in {environment}')
        return func.HttpResponse(
            f'No module found for {module} in {environment}',
            status_code=400
        )
    
    manifest = latest_module['manifest']
    print(f'manifest={manifest}')


    
    if ev not in ['apply', 'destroy']:
        return func.HttpResponse(f'Invalid event type ({ev})', status_code=400)

    project_name = f"infrabridge-worker-{region}-{environment}"

    module_envs = []
    for key, value in spec.items():
        module_envs.append({
            "name": f"PARAM_TF_VAR_{camel_to_snake(key)}",
            "value": value,
            "type": "PLAINTEXT"
        })
    tf_vars_json = json.dumps(module_envs)
    
    # response = codebuild.start_build(
    #     projectName=project_name,
    #     environmentVariablesOverride=[
    #         {
    #             "name": "DEPLOYMENT_ID",
    #             "value": deployment_id,
    #             "type": "PLAINTEXT"
    #         },
    #         {
    #             "name": "EVENT",
    #             "value": ev,
    #             "type": "PLAINTEXT"
    #         },
    #         {
    #             "name": "SIGNAL",
    #             "value": json.dumps(get_signal_dict(codebuild=True)),
    #             "type": "PLAINTEXT"
    #         }
    #     ] + module_envs,
    #     sourceLocationOverride=source_location,
    #     sourceVersion="",
    #     sourceTypeOverride=source_type,
    #     logsConfigOverride={
    #         'cloudWatchLogs': {
    #             'status': 'ENABLED',
    #             'groupName': f'/aws/codebuild/{project_name}',
    #             'streamName': deployment_id,
    #         },
    #     }
    # )

    url = f"https://dev.azure.com/{organization}/{project}/_apis/build/builds?api-version=6.0"
    # pat = 'fgbtfsjfqczfreblvt5qbsvbzfmkkumhkr5bxrgsqjtjwbpv5x7a'  # Replace with your PAT or retrieve from Key Vault
    encoded_pat = str(base64.b64encode(bytes(':' + pat, 'ascii')), 'ascii')
    headers = {
        'Authorization': 'Basic ' + encoded_pat,
        'Content-Type': 'application/json'
    }
    payload = {
        "definition": {
            "id": pipeline_id,
        },
        "parameters": json.dumps({
            "EVENT": ev,
            "MODULE_NAME": module,
            "DEPLOYMENT_ID": deployment_id,
            "TF_STORAGE_ACCOUNT": tf_storage_account,
            "TF_CONTAINER": tf_container,
            "TF_STORAGE_ACCESS_KEY": tf_storage_access_key,
            "ENVIRONMENT": environment,
            "REGION": region.replace(" ", ""),
            "TF_DYNAMODB_TABLE": tf_dynamodb_table,
            "TF_VARS_JSON": tf_vars_json,
            "PROJECT_NAME": project_name,
        })
    }

    response = requests.post(url, headers=headers, json=payload)

    if response.status_code == 200:
        return func.HttpResponse(f"Pipeline triggered successfully.", status_code=200)
    else:
        return func.HttpResponse(f"Failed to trigger pipeline: {response.text}", status_code=response.status_code)


def camel_to_snake(name):
    import re
    # Insert an underscore before each uppercase letter and convert to lowercase
    return re.sub(r'(?<!^)(?=[A-Z])', '_', name).lower()

def generate_id(exclude_chars="", length=1):
    chars = string.ascii_letters + string.digits
    valid_chars = ''.join(c for c in chars if c not in exclude_chars)
    return ''.join(random.choice(valid_chars) for _ in range(length))


def get_latest_module(module, environment):
    print(f'module = {module}, environment = {environment} !')
    entries = get_latest_entries(module, environment, 1)
    return entries[0] if entries else None

# def get_latest_module_entries(module, environment, num_entries):
#     print(f'module_table_name={module_table_name}')
#     modules_table = dynamodb.Table(module_table_name)
#     response = modules_table.query(
#         IndexName='VersionEnvironmentIndex',
#         KeyConditionExpression=Key('module').eq(module),
#         ScanIndexForward=False,  # False for descending order
#         Limit=num_entries,  # Return the latest n entries
#         FilterExpression=Attr('environment_version').begins_with(f'{environment}#'),
#     )
#     print(response)

#     if response['Items']:
#         return response['Items']
#     else:
#         return []  # No entries found for the deployment_id

from datetime import datetime, timedelta

def get_latest_entries(module, environment, num_entries):
    # Query for the latest entry based on the deployment_id
    prefix = f"{environment}|"
    next_char = chr(ord(prefix[-1]) + 1)  # Find the next character in the ASCII table
    end_of_range = prefix[:-1] + next_char  # Replace the last character with its successor

    filter_query = f"PartitionKey eq '{module}' and RowKey ge '{prefix}' and RowKey lt '{end_of_range}'"
    logging.info(f"Filter query: {filter_query}")

    try:
        entities = list(modules_table_client.query_entities(query_filter=filter_query, results_per_page=num_entries))
        sorted_entities = sorted(entities, key=lambda x: datetime.strptime(x['timestamp'], '%Y-%m-%dT%H:%M:%SZ'), reverse=True)[:num_entries]
        return sorted_entities
    except Exception as e:
        logging.error(f"An error occurred: {e}")
        return []  # No entries found for the deployment_id

# def get_sas_token(container_name, account_key):
#     from datetime import datetime, timedelta
#     from azure.storage.blob import BlobServiceClient, generate_blob_sas, BlobSasPermissions
#     account_name = 'examplefuncstormar2'
#     # account_key = 'your_account_key_here'
#     blob_name = 'module.zip'

#     sas_token = generate_blob_sas(account_name=account_name,
#                                 container_name=container_name,
#                                 blob_name=blob_name,
#                                 account_key=account_key,
#                                 permission=BlobSasPermissions(read=True),
#                                 expiry=datetime.utcnow() + timedelta(minutes=10))

#     print(f"SAS Token: {sas_token}")