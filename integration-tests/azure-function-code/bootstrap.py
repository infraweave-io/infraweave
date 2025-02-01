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


def bootstrap_buckets():
    conn_str = os.environ["AZURITE_CONNECTION_STRING"]
    blob_service_client = BlobServiceClient.from_connection_string(conn_str)

    container_name = "modules"
    blob_service_client.create_container(container_name)
