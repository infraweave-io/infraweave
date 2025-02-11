import os
import base64
from datetime import datetime, timedelta
import json
import uuid
import azure.functions as func
import logging
from azure.identity import DefaultAzureCredential

from bootstrap import bootstrap_tables, bootstrap_buckets

COSMOS_KEY = os.environ.get("COSMOS_KEY")

app = func.FunctionApp(http_auth_level=func.AuthLevel.ANONYMOUS) # only for testing

from azure.mgmt.containerinstance import ContainerInstanceManagementClient
from azure.mgmt.containerinstance.models import (
    ContainerGroup,
    Container,
    ContainerGroupNetworkProtocol,
    ContainerPort,
    ResourceRequests,
    ResourceRequirements,
    OperatingSystemTypes
)

@app.function_name(name="generic_api")
@app.route(route="api")
def handler(req: func.HttpRequest) -> func.HttpResponse:
    try:
        req_body = req.get_json()
    except ValueError:
        return func.HttpResponse("Invalid JSON body.", status_code=400)
    logging.info("req_body:")
    logging.info(req_body)

    event = req_body.get('event')
    import traceback
    try:
        if event == 'bootstrap_tables':
            bootstrap_tables()
            return func.HttpResponse(json.dumps({"result": "success"}), status_code=200)
        if event == 'bootstrap_buckets':
            bootstrap_buckets()
            return func.HttpResponse(json.dumps({"result": "success"}), status_code=200)
        
        if event == 'insert_db':
            return insert_db(req)
        elif event == 'read_db':
            return read_db(req)
        elif event == 'start_runner':
            return start_runner(req)
        elif event == 'upload_file_base64':
            return upload_file_base64(req)
        elif event == 'read_logs':
            return read_logs(req)
        elif event == 'generate_presigned_url':
            return generate_presigned_url(req)
        elif event == 'transact_write':
            return transact_write(req)
        else:
            return func.HttpResponse(json.dumps({"result":f"Invalid event type ({event})"}), status_code=400)
    except Exception as e:
        tb = traceback.format_exc()
        return func.HttpResponse(json.dumps({"result":f"Api error: {e}", "tb": tb}), status_code=500)
    
def transact_write(req: func.HttpRequest) -> func.HttpResponse:
    from azure.cosmos import CosmosClient, exceptions

    try:
        req_body = req.get_json()
    except ValueError:
        return func.HttpResponse("Invalid JSON body.", status_code=400)
    
    
    COSMOS_DB_ENDPOINT = os.getenv("COSMOS_DB_ENDPOINT")
    COSMOS_DB_DATABASE = os.getenv("COSMOS_DB_DATABASE")

    # credential = DefaultAzureCredential()
    credential = COSMOS_KEY
    # client = CosmosClient(COSMOS_DB_ENDPOINT, credential=credential)
    client = CosmosClient(
        COSMOS_DB_ENDPOINT,
        credential=credential,
        connection_mode="Gateway",
        consistency_level="Session",
        enable_endpoint_discovery=False,
        connection_verify=False
    )

    database = client.get_database_client(COSMOS_DB_DATABASE)
    responses = []

    for item in req_body['items']:
        try:
            if 'Put' in item:
                container_name = item['Put']['TableName']
                container = database.get_container_client(container_name)
                put_item = item['Put']['Item']
                put_item.update({'id': get_id(put_item)}) # Reserved field that should not be used in InfraWeave rows, but is required by Cosmos DB
                
                response = container.upsert_item(put_item)
                responses.append({"operation": "Put", "status": "Success", "item_id": put_item["id"]})
                
            elif 'Delete' in item:
                container_name = item['Delete']['TableName']
                container = database.get_container_client(container_name)
                delete_key = item['Delete']['Key']
                
                container.delete_item(item=delete_key['id'], partition_key=delete_key['partition_key'])
                responses.append({"operation": "Delete", "status": "Success", "item_id": delete_key["id"]})

        except exceptions.CosmosHttpResponseError as e:
            responses.append({
                # "item_id": item.get('Put', {}).get('Item', {}).get('id', "N/A") or item.get('Delete', {}).get('Key', {}).get('id', "N/A"),
                "error": str(e)
            })
    return func.HttpResponse(
        body=json.dumps(responses),
        status_code=200,
        mimetype="application/json"
    )


def generate_presigned_url(req: func.HttpRequest) -> func.HttpResponse:
    try:
        req_body = req.get_json()
    except ValueError:
        return func.HttpResponse("Invalid JSON body.", status_code=400)
    
    from azure.storage.blob import BlobServiceClient, generate_blob_sas, BlobSasPermissions

    req_body = req.get_json()
    payload = req_body.get('data')
    container_name = payload.get("bucket_name")  # Equivalent to bucket_name in AWS
    blob_name = payload.get("key")
    expires_in = payload.get("expires_in", 3600)


    sas_expiry = datetime.utcnow() + timedelta(seconds=expires_in)

    # blob_service_client = BlobServiceClient(
    #     account_url=f"https://{account_name}.blob.core.windows.net",
    #     credential=DefaultAzureCredential()
    # )
    conn_str = os.environ["AZURITE_CONNECTION_STRING"]
    blob_service_client = BlobServiceClient.from_connection_string(conn_str)

    account_name = blob_service_client.account_name #os.getenv("STORAGE_ACCOUNT_NAME") #or "storageAccount1"
    
    # # Get the user delegation key using AAD credentials
    # user_delegation_key = blob_service_client.get_user_delegation_key(
    #     key_start_time=datetime.utcnow(),
    #     key_expiry_time=sas_expiry
    # )
    account_key = blob_service_client.credential.account_key 

    sas_token = generate_blob_sas(
        account_name=account_name,
        container_name=container_name,
        blob_name=blob_name,
        permission=BlobSasPermissions(read=True),  # Use read permissions for download access
        expiry=sas_expiry,
        account_key=account_key,
        # user_delegation_key=user_delegation_key,
    )

    # blob_url = f"https://{account_name}.blob.core.windows.net/{container_name}/{blob_name}?{sas_token}"
    blob_url = f"http://127.0.0.1:10000/{account_name}/{container_name}/{blob_name}?{sas_token}" # it will be pulled from the host machine in the test

    return func.HttpResponse(
        json.dumps({"url": blob_url}),
        status_code=200,
        mimetype="application/json"
    )

def start_runner(req: func.HttpRequest) -> func.HttpResponse:

    return func.HttpResponse(json.dumps({"result":"Would have been started", "job_id": "test-job-id"}), status_code=200)

    from azure.identity import DefaultAzureCredential
    credential = DefaultAzureCredential()

    try:
        req_body = req.get_json()
    except ValueError:
        return func.HttpResponse("Invalid JSON body.", status_code=400)

    image_name = req_body.get("image_name", "mcr.microsoft.com/azuredocs/aci-helloworld")  # Default if not provided

    # Subscription ID from environment variable
    subscription_id = os.getenv("SUBSCRIPTION_ID")
    resource_group_name = os.getenv("RESOURCE_GROUP_NAME")  # Make sure this environment variable is set

    # Initialize the Container Instance Management Client
    client = ContainerInstanceManagementClient(credential, subscription_id)

    app = func.FunctionApp(http_auth_level=func.AuthLevel.FUNCTION)


    logging.info('Python HTTP trigger function processed a request.')

    # Generate a unique container group name
    container_group_name = f"ephemeral-task-{str(uuid.uuid4())[:8]}"


    try:
        # Define container configuration
        container_resource_requirements = ResourceRequirements(
            requests=ResourceRequests(memory_in_gb=1.5, cpu=1.0)
        )
        container = Container(
            name="runner",
            image=image_name,
            resources=container_resource_requirements,
            ports=[ContainerPort(port=80)],
            environment_variables=[
                {"name": "ENVIRONMENT", "value": "dev"},
                # {"name": "REGION", "value": os.getenv("REGION")}
            ]
        )

        # Define container group configuration
        container_group = ContainerGroup(
            location=os.getenv("LOCATION"),
            containers=[container],
            os_type=OperatingSystemTypes.Linux,
            restart_policy="Never"
        )

        # Start the container group
        client.container_groups.begin_create_or_update(
            resource_group_name=resource_group_name,
            container_group_name=container_group_name,
            container_group=container_group
        )
        
        logging.info("ACI task started successfully.")
        return func.HttpResponse(f"ACI task started successfully.", status_code=200)

    except Exception as e:
        logging.error(f"Error starting ACI task: {e}")
        return func.HttpResponse(f"Error starting ACI task {e}", status_code=500)

# needs to be cleaned up manually, exmaple: (max 100 containers regardless of state (running, stopped, etc.))

# #!/bin/bash
# # Lists all stopped containers and deletes them

# resource_group="your-resource-group"
# containers=$(az container list --resource-group $resource_group --query "[?provisioningState=='Succeeded' && instanceView.state=='Stopped'].name" -o tsv)

# for container in $containers; do
#   echo "Deleting container group: $container"
#   az container delete --resource-group $resource_group --name $container --yes
# done

import urllib.parse

def get_id(body):
    return urllib.parse.quote(f'{body.get("PK")}/{body.get("SK")}', safe="")

def insert_db(req: func.HttpRequest) -> func.HttpResponse:
    from azure.cosmos import CosmosClient, exceptions
    
    COSMOS_DB_ENDPOINT = os.getenv("COSMOS_DB_ENDPOINT")
    COSMOS_DB_DATABASE = os.getenv("COSMOS_DB_DATABASE")

    try:
        req_body = req.get_json()
    except ValueError:
        return func.HttpResponse("Invalid JSON body.", status_code=400)

    container_name = req_body.get('table')
    item = req_body.get('data')
    item.update({'id': get_id(req_body)}) # Reserved field that should not be used in InfraWeave rows, but is required by Cosmos DB

    # credential = DefaultAzureCredential()
    credential = COSMOS_KEY
    # client = CosmosClient(COSMOS_DB_ENDPOINT, credential=credential)
    client = CosmosClient(
        COSMOS_DB_ENDPOINT,
        credential=credential,
        connection_mode="Gateway",
        consistency_level="Session",
        enable_endpoint_discovery=False,
        connection_verify=False
    )

    database = client.get_database_client(COSMOS_DB_DATABASE)
    container = database.get_container_client(container_name)

    try:
        response = container.upsert_item(body=item)
        return func.HttpResponse(json.dumps(response), status_code=200)
    except exceptions.CosmosHttpResponseError as e:
        print(f'Error inserting item: {e}')
        return func.HttpResponse(f'Error inserting item: {e}', status_code=500)


def read_db(req: func.HttpRequest) -> func.HttpResponse:
    from azure.cosmos import CosmosClient, exceptions
    
    COSMOS_DB_ENDPOINT = os.getenv("COSMOS_DB_ENDPOINT")
    COSMOS_DB_DATABASE = os.getenv("COSMOS_DB_DATABASE")

    try:
        req_body = req.get_json()
    except ValueError:
        return func.HttpResponse("Invalid JSON body.", status_code=400)


    container_name = req_body.get('table')
    query = req_body.get('data').get('query')

    # credential = DefaultAzureCredential()
    credential = COSMOS_KEY
    # client = CosmosClient(COSMOS_DB_ENDPOINT, credential=credential)
    client = CosmosClient(
        COSMOS_DB_ENDPOINT,
        credential=credential,
        connection_mode="Gateway",
        consistency_level="Session",
        enable_endpoint_discovery=False,
        connection_verify=False
    )

    database = client.get_database_client(COSMOS_DB_DATABASE)
    container = database.get_container_client(container_name)

    try:
        items = list(container.query_items(
            query=query,
            enable_cross_partition_query=True
        ))
        import json
        logging.info(f"Read operation succeeded, found {len(items)} items.")
        return func.HttpResponse(json.dumps(items), status_code=200)
    except exceptions.CosmosHttpResponseError as e:
        print(f'Error querying items: {e}')
        return func.HttpResponse(f"error querying: {e}", status_code=500)

def upload_file_base64(req: func.HttpRequest) -> func.HttpResponse:
    from azure.storage.blob import BlobServiceClient, BlobClient, ContainerClient
    
    try:
        req_body = req.get_json()
    except ValueError:
        return func.HttpResponse("Invalid JSON body.", status_code=400)

    
    account_name = os.getenv("STORAGE_ACCOUNT_NAME")
    # container_name = os.getenv("CONTAINER_NAME")
    # blob_service_client = BlobServiceClient(
    #     account_url=f"https://{account_name}.blob.core.windows.net",
    #     credential=DefaultAzureCredential()
    # )
    conn_str = os.environ["AZURITE_CONNECTION_STRING"]
    blob_service_client = BlobServiceClient.from_connection_string(conn_str)

    payload = req_body.get('data')
    container_name = payload.get('bucket_name').replace('_', '')
    base64_body = payload.get('base64_content')
    blob_name = payload.get('key')
    binary_body = base64.b64decode(base64_body)
    blob_client = blob_service_client.get_blob_client(container=container_name, blob=blob_name)
    blob_client.upload_blob(binary_body, overwrite=True)
    print(f"Blob {blob_name} uploaded to container {container_name} successfully.")
    response_body = {
        "status": f"Blob {blob_name} uploaded to container {container_name} successfully."
    }
    return func.HttpResponse(
        json.dumps(response_body),
        status_code=200,
        mimetype="application/json"
    )

def read_logs(req: func.HttpRequest) -> func.HttpResponse:
    response_body = {
        "status": "success",
        "events": [
            {"message": "Some log message 1"},
            {"message": "Some log message 2"},
            {"message": "Some log message 3"},
        ]
    }
    return func.HttpResponse(
        json.dumps(response_body),
        status_code=200,
        mimetype="application/json"
    )