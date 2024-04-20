import boto3
import os
from source.generator import run_and_zip

DOCS_BUCKET = os.environ['DOCS_BUCKET']

def upload_to_s3(file_name, bucket, object_name=None):
    if object_name is None:
        object_name = file_name

    s3_client = boto3.client('s3')
    response = s3_client.upload_file(file_name, bucket, object_name)
    return response

def handler(event, context):
    print("Running docs generator...")

    zip_path = '/tmp/webpage_archive'
    run_and_zip(zip_path)
    
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