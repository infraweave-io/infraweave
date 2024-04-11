from datetime import datetime
import os
import time
import yaml
import json
import logging
from datetime import datetime
from jsonschema import validate
from jsonschema.exceptions import ValidationError
from packaging import version as semver

import azure.functions as func
from azure.data.tables import TableServiceClient
from azure.core.exceptions import ResourceNotFoundError

# from openapi_core import Spec

# spec = Spec.from_file_path('openapi.json')

module_table_name = os.environ.get('STORAGE_TABLE_MODULES_TABLE_NAME')
module_connection_string = os.getenv("AZURE_MODULES_TABLE_CONN_STR")
module_table_service = TableServiceClient.from_connection_string(conn_str=module_connection_string)
modules_table_client = module_table_service.get_table_client(table_name=module_table_name)


environment_table_name = os.environ.get('STORAGE_TABLE_ENVIRONMENTS_TABLE_NAME')
environment_connection_string = os.getenv("AZURE_ENVIRONMENTS_TABLE_CONN_STR")
environment_table_service = TableServiceClient.from_connection_string(conn_str=environment_connection_string)
environment_table_client = environment_table_service.get_table_client(table_name=environment_table_name)

def main(req: func.HttpRequest) -> func.HttpResponse:
    
    try:
        # Attempt to get JSON body
        event = req.get_json()
    except ValueError:
        # If there is no JSON body, or if JSON is invalid, set req_body to None or handle as needed
        return func.HttpResponse(
            "Please pass JSON in the request body",
            status_code=400
        )
    
    # module = event.get('module')
    event_type = event.get('event')
    environment = event.get('environment')

    if event_type == 'insert':
        yaml_manifest = event.get('manifest')
        # read schema from file
        with open('schema_module.yaml', 'r') as file:
            schema = yaml.safe_load(file) # Important https://pyyaml.org/wiki/PyYAMLDocumentation#LoadingYAML
        try:
            manifest = yaml.safe_load(yaml_manifest) # Important https://pyyaml.org/wiki/PyYAMLDocumentation#LoadingYAML
            validate(instance=manifest, schema=schema)
            print("Manifest is valid")
        except ValidationError as e:
            print("Manifest is invalid")
            return func.HttpResponse(
                f"Manifest is invalid: {e}",
                status_code=400
            )
        module = manifest['metadata']['name']
        # environment = manifest['spec']['environment']
        module_name = manifest['spec']['moduleName']
        version = manifest['spec']['version']
        source = manifest['spec']['source']
        parameters = manifest['spec']['parameters']
        force = event.get('force', False)
        description = event.get('description', '')
        reference = event.get('reference', '')

        # Check if the module already exists
        latest_entries = get_latest_entries(
            module=module, 
            environment=environment,
            num_entries= 1,
        )
        logging.info(latest_entries)
        parsed_version_this = semver.parse(version)

        if len(latest_entries) > 0:
            latest_version = latest_entries[0]['version']
            if latest_version == version and not force:
                return f"Module {module} ({version}) already exists!"
            else:
                logging.info(f"Module {module} ({version}) already exists, but force is set to True. Inserting new version...")

            parsed_version_latest = semver.parse(latest_version)

            if parsed_version_this < parsed_version_latest:
                return f"Version {version} is older than the latest version {latest_version} for module {module}!"

        logging.info(f"Inserting module {module} ({version})")
        insert_module(
            module=module,
            module_name=module_name,
            version=version,
            manifest=manifest,
            environment=environment,
            description=description,
            reference=reference,
            timestamp=datetime.utcnow().replace(microsecond=0).isoformat() + 'Z',
        )
        return f"Module {module} ({version}) inserted successfully!"
        return(f'Manifest: {manifest}')
    elif event_type == 'get_latest':
        num_entries = event.get('num_entries', 1)
        environment = event.get('environment')
        latest_entries = get_latest_entries(
            module=module, 
            environment=environment,
            num_entries= 1,
        )
        print(latest_entries)
        return latest_entries
    elif event_type == 'list_latest':
        environment = event.get('environment')
        latest_entries = get_latest_modules(
            environment=environment,
        )
        print(latest_entries)
        return json.dumps(latest_entries)
    elif event_type == 'get_module':
        module = event.get('module')
        version = event.get('version')
        latest_entries = get_module_version(
            version=version,
            module=module,
        )
        print(latest_entries)
        return json.dumps(latest_entries)
    elif event_type == 'list_environments':
        environments = get_environments()
        print(environments)
        return json.dumps(environments)
    else:
        return "Invalid event type"


def insert_module(module, module_name, version, environment, manifest, timestamp, description, reference):
    epoch = int(time.time())
    environment_table_client.upsert_entity(
        entity={
            'PartitionKey': environment,
            'RowKey': '',
            'environment': environment,
            'last_activity_epoch': epoch,
        }
    )
    # Ensure manifest is a JSON string if it's not already
    manifest_json = json.dumps(manifest) if isinstance(manifest, dict) else manifest
    # Ensure timestamp is formatted as a string if it's a datetime object
    timestamp_str = timestamp.isoformat() if isinstance(timestamp, datetime) else timestamp

    entity = {
        'PartitionKey': module,
        'RowKey': f"{environment}|{zero_pad_semver(version)}",
        'module': module,
        'module_name': module_name,
        'version': version,
        'environment': environment,
        'manifest': manifest_json,
        'timestamp': timestamp_str,
        'description': description,
        'reference': reference,
    }
    response = modules_table_client.create_entity(entity)
    logging.info(f"Inserted module {module} ({version}) into table")
    logging.info(f"Response: {response}")
    # Insert the entity into the table to keep track on all latest modules (workaround is to overwrite 
    # the same entry with empty RowKey everytime due to missing indexes in Azure Table Storage)
    entity['RowKey'] = ''
    response = modules_table_client.create_entity(entity)
    return response

def get_latest_entries(module, environment, num_entries=999):
    # Query for the latest entry based on the deployment_id
    prefix = f"{environment}|"
    next_char = chr(ord(prefix[-1]) + 1)  # Find the next character in the ASCII table
    end_of_range = prefix[:-1] + next_char  # Replace the last character with its successor

    if isinstance(module, str):
        module_query = f"PartitionKey eq '{module}'"
    elif isinstance(module, list):
        module_query = ' or '.join([f"PartitionKey eq '{module}'" for module in module])

    filter_query = f"{module_query} and RowKey ge '{prefix}' and RowKey lt '{end_of_range}'"
    logging.info(f"Filter query: {filter_query}")

    try:
        entities = list(modules_table_client.query_entities(query_filter=filter_query, results_per_page=num_entries))
        logging.info(entities)
        sorted_entities = sorted(entities, key=lambda x: datetime.strptime(x['timestamp'], '%Y-%m-%dT%H:%M:%SZ'), reverse=True)[:num_entries]
        return sorted_entities
    except Exception as e:
        logging.error(f"An error occurred: {e}")
        return []  # No entries found for the deployment_id


def get_all_modules():
    # Query for all entries with empty RowKey which is the workaround to get all modules
    filter_query = f"RowKey eq ''"
    logging.info(f"Filter query: {filter_query}")

    try:
        entities = list(modules_table_client.query_entities(query_filter=filter_query))
        logging.info(entities)
        return entities
        # return [entity['PartitionKey'] for entity in entities]
    except Exception as e:
        logging.error(f"An error occurred: {e}")
        return []  # No entries found for the deployment_id


def get_latest_modules(environment):
    all_modules = get_all_modules() # Get all modules from the table by querying for empty RowKey
    return all_modules

def get_module_version(module, version):
    # Construct a filter query to find entities with matching module and version
    filter_query = f"PartitionKey eq '{module}' and version eq '{version}'"
    try:
        entities = modules_table_client.query_entities(query_filter=filter_query)
        results = list(entities)
        logging.info(results)
        if results:
            return results[0]
        else:
            return None
    except Exception as e:
        logging.error(f"An error occurred: {e}")
        return None  # No entries found for the given module and version


def get_environments():
    try:
        entities = environment_table_client.list_entities()
        environments = []

        for entity in entities:
            if 'last_activity_epoch' in entity:
                entity['last_activity_epoch'] = int(entity['last_activity_epoch'])
            environments.append(entity)

        return environments
    except Exception as e:
        print(f"An error occurred: {e}")
        return []  # No entries found

    
def zero_pad_semver(ver_str, pad_length=3):
    """
    Zero-pads the major, minor, and patch components of a semantic version.
    Preserves additional version information (e.g., pre-release, build metadata).

    Args:
    - ver_str (str): The semantic version string.
    - pad_length (int): The length to zero-pad numbers to. Default is 3.

    Returns:
    - str: The zero-padded semantic version string.
    """

    # Parse the version string
    version = semver.parse(ver_str)

    # Zero-pad the major, minor, and patch components
    major = str(version.major).zfill(pad_length)
    minor = str(version.minor).zfill(pad_length)
    patch = str(version.micro).zfill(pad_length)  # `micro` is the patch version

    # Reconstruct the version string with zero-padding and any additional info
    reconstructed = f"{major}.{minor}.{patch}"

    # Append pre-release and build metadata if present
    if version.pre:
        reconstructed += f"-{version.pre}"
    if version.local:
        reconstructed += f"+{version.local}"

    return reconstructed