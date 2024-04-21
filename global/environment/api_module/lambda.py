from datetime import datetime
import os
import time
import boto3
import yaml
import json
from jsonschema import validate
from jsonschema.exceptions import ValidationError
from packaging import version as semver
from boto3.dynamodb.conditions import Key

# from openapi_core import Spec

# spec = Spec.from_file_path('openapi.json')

dynamodb = boto3.resource('dynamodb')
modules_table_name = os.environ.get('DYNAMODB_MODULES_TABLE_NAME')
module_table_name = os.environ.get('DYNAMODB_MODULES_TABLE_NAME')
modules_table = dynamodb.Table(modules_table_name)

def handler(event, context):
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
    environments_table_name = os.environ.get('DYNAMODB_ENVIRONMENTS_TABLE_NAME')
    environments_table = dynamodb.Table(environments_table_name)
    epoch = int(time.time())
    environments_table.put_item(
        Item={
            'environment': environment,
            'last_activity_epoch': epoch,
        }
    )
    response = modules_table.put_item(
        Item={
            'module': module,
            'environment_version': f"{environment}#{zero_pad_semver(version)}",
            'module_name': module_name,
            'version': version,
            'environment': environment,
            'manifest': manifest,
            'timestamp': timestamp,
            'description': description,
            'reference': reference,
        }
    )
    return response


def get_latest_entries(module, environment, num_entries):
    # Query for the latest entry based on the deployment_id
    response = modules_table.query(
        KeyConditionExpression='#mod = :module_val',
        FilterExpression='#env = :env_val',
        ExpressionAttributeNames={
            '#mod': 'module',
            '#env': 'environment'
        },
        ExpressionAttributeValues={
            ':module_val': module,
            ':env_val': environment
        },
        ScanIndexForward=False,  # False to sort results by range key in descending order
        Limit=num_entries  # Return the latest n entries
    )

    if response['Items']:
        return response['Items']
    else:
        return []  # No entries found for the deployment_id


def get_latest_modules(environment):
    response = modules_table.query(
        IndexName='EnvironmentModuleVersionIndex',  # Adjusted GSI name
        KeyConditionExpression=Key('environment').eq(environment),
        ScanIndexForward=False  # False to sort in descending order
    )
    
    latest_modules = {}
    for item in response['Items']:
        module_name = item['module']  # Assuming you have logic to extract module name from the composite key
        if module_name not in latest_modules:
            latest_modules[module_name] = item  # Assumes first occurrence is the latest version due to sorting
    
    return list(latest_modules.values())


def get_module_version(module, version):
    modules_table = dynamodb.Table(module_table_name)
    response = modules_table.query(
        IndexName='VersionEnvironmentIndex',
        KeyConditionExpression=Key('module').eq(module) & Key('version').eq(version),
        ScanIndexForward=False,  # False for descending order
        Limit=1  # Return the latest entry
    )
    return response['Items'][0] if response['Items'] else None


def get_environments():
    environments_table_name = os.environ.get('DYNAMODB_ENVIRONMENTS_TABLE_NAME')
    environments_table = dynamodb.Table(environments_table_name)
    response = environments_table.scan(
        # FilterExpression=Key('environment').eq(environment)
    )
    for item in response['Items']:
        item['last_activity_epoch'] = int(item['last_activity_epoch'])

    return response['Items']

    
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