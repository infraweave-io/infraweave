from datetime import datetime
import os
import time
import yaml
import json
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
            schema = yaml.safe_load(file)
        try:
            manifest = yaml.safe_load(yaml_manifest)
            validate(instance=manifest, schema=schema)
            print("Manifest is valid")
        except ValidationError as e:
            print("Manifest is invalid")
            return
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
        print(latest_entries)
        parsed_version_this = semver.parse(version)

        if len(latest_entries) > 0:
            latest_version = latest_entries[0]['version']
            if latest_version == version and not force:
                return f"Module {module} ({version}) already exists!"
            else:
                print(f"Module {module} ({version}) already exists, but force is set to True. Inserting new version...")

            parsed_version_latest = semver.parse(latest_version)

            if parsed_version_this < parsed_version_latest:
                return f"Version {version} is older than the latest version {latest_version} for module {module}!"

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
    # epoch = int(time.time())
    # environments_table.put_item(
    #     Item={
    #         'environment': environment,
    #         'last_activity_epoch': epoch,
    #     }
    # )
    response = modules_table_client.create_entity({
        'PartitionKey': module, # Azure needs PartitionKey instead of naming it 'module' like in AWS
        'RowKey': f"{environment}#{zero_pad_semver(version)}",  # Azure needs RowKey instead of naming it 'environment_version' like in AWS
        'module_name': module_name,
        'version': version,
        'environment': environment,
        'manifest': manifest,
        'timestamp': timestamp,
        'description': description,
        'reference': reference,
    })
    return response

def get_latest_entries(module, environment, num_entries):
    # Query for the latest entry based on the deployment_id
    filter_query = f"PartitionKey eq '{module}' and RowKey eq '{environment}'"
    try:
        entities = modules_table_client.query_entities(query_filter=filter_query, results_per_page=num_entries)
        sorted_entities = sorted(entities, key=lambda x: x['Timestamp'], reverse=True)[:num_entries]
        return sorted_entities
    except Exception as e:
        print(f"An error occurred: {e}")
        return []  # No entries found for the deployment_id


def get_latest_modules(environment):
    filter_query = f"PartitionKey eq '{environment}'"
    try:
        entities = modules_table_client.query_entities(query_filter=filter_query)
        
        latest_modules = {}
        for entity in entities:
            # Assuming ModuleName is stored and can be used to distinguish modules
            module_name = entity['ModuleName']
            
            # If module_name is not already in latest_modules, or if the current entity's timestamp is newer
            if module_name not in latest_modules or entity['Timestamp'] > latest_modules[module_name]['Timestamp']:
                latest_modules[module_name] = entity

        return list(latest_modules.values())
    except Exception as e:
        print(f"An error occurred: {e}")
        return []  # No entries found for the given environment


def get_module_version(module, version):
    # Construct a filter query to find entities with matching module and version
    # If version is part of RowKey or another property, adjust accordingly
    filter_query = f"PartitionKey eq '{module}'"
    try:
        entities = modules_table_client.query_entities(query_filter=filter_query)
        # Assuming 'version' is a property of the entities
        version_entities = [entity for entity in entities if entity.get('version') == version]
        
        # Sort entities by Timestamp to find the latest one, if multiple are found
        if version_entities:
            latest_entity = sorted(version_entities, key=lambda x: x['Timestamp'], reverse=True)[0]
            return latest_entity
        else:
            return None
    except Exception as e:
        print(f"An error occurred: {e}")
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