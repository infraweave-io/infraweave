import json
import os
from azure.cosmos import CosmosClient, PartitionKey
from azure.storage.blob import BlobServiceClient

COSMOS_DB_ENDPOINT = os.environ.get("COSMOS_DB_ENDPOINT")
COSMOS_KEY = os.environ.get("COSMOS_KEY")
COSMOS_DB_NAME = os.environ.get("COSMOS_DB_DATABASE")

AZURE_STORAGE_CONNECTION_STRING = os.environ.get("AZURE_STORAGE_CONNECTION_STRING")


def bootstrap_tables():
    client = CosmosClient(
        COSMOS_DB_ENDPOINT,
        credential=COSMOS_KEY,
        connection_mode="Gateway",
        consistency_level="Session",
        enable_endpoint_discovery=False,
        connection_verify=False
    )

    database = client.create_database_if_not_exists(id=COSMOS_DB_NAME)

    events_container_name = "events"
    modules_container_name = "modules"
    policies_container_name = "policies"
    change_records_container_name = "change-records"
    deployments_container_name = "deployments"
    config_container_name = "config"

    # Events container
    database.create_container_if_not_exists(
        id=events_container_name,
        partition_key=PartitionKey(path="/PK"),
        offer_throughput=400
    )

    # Modules container
    database.create_container_if_not_exists(
        id=modules_container_name,
        partition_key=PartitionKey(path="/PK"),
        offer_throughput=400
    )

    # Policies container
    database.create_container_if_not_exists(
        id=policies_container_name,
        partition_key=PartitionKey(path="/PK"),
        offer_throughput=400
    )

    # ChangeRecords container
    database.create_container_if_not_exists(
        id=change_records_container_name,
        partition_key=PartitionKey(path="/PK"),
        offer_throughput=400
    )

    # Deployments container
    database.create_container_if_not_exists(
        id=deployments_container_name,
        partition_key=PartitionKey(path="/PK"),
        offer_throughput=400
    )

    # Configs container
    database.create_container_if_not_exists(
        id=config_container_name,
        partition_key=PartitionKey(path="/PK"),
        offer_throughput=400
    )

    container = database.get_container_client(config_container_name)

    # Insert config item (uses UDF in real code and azure function)
    all_regions_config_item = {
        "id": "all_regions",
        "PK": "all_regions",
        "data": {
            "regions": ["eastus"]
        }
    }
    container.upsert_item(all_regions_config_item)
    project_map_config_item = {
        "id": "project_map",
        "PK": "project_map",
        "data": {
            "some-org/deploy-project-1": {
                "project_id": "123400000000"
            },
            "some-org/deploy-project-1": {
                "project_id": "987600000000"
            }
        }
    }
    container.upsert_item(project_map_config_item)

def bootstrap_buckets():
    conn_str = os.environ["AZURITE_CONNECTION_STRING"]
    blob_service_client = BlobServiceClient.from_connection_string(conn_str)

    blob_service_client.create_container("modules")
    blob_service_client.create_container("policies")
    blob_service_client.create_container("change-records")
    blob_service_client.create_container("providers")
    blob_service_client.create_container("tf-state")
