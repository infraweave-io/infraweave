import json
import boto3

def return_html_table(resource_groups, tagging_client, resource_groups_name):
        # HTML header
    html = f"<html><body><h2>Resource Group: {resource_groups_name}</h2><table border='1'>"
    html += "<tr><th>Resource ARN</th><th>Resource Type</th><th>Resource Name</th><th>Deployment Id</th><th>Other tags</th></tr>"

    try:
        # Fetch resources and their types
        group_resources = resource_groups.list_group_resources(GroupName=resource_groups_name, MaxResults=50)
        resources = group_resources.get('ResourceIdentifiers', [])
        
        # Fetch tags for the ARNs gathered
        arns = [res['ResourceArn'] for res in resources]
        if arns:
            tagging_info = tagging_client.get_resources(ResourceARNList=arns)
            
            # Process and display each resource
            for res in resources:
                resource_arn = res['ResourceArn']
                resource_type = res['ResourceType']

                all_tags = [item['Tags'] for item in tagging_info['ResourceTagMappingList'] if item['ResourceARN'] == resource_arn][0]

                deployment_id = next((tag['Value'] for tag in all_tags if tag['Key'] == 'DeploymentId'), 'N/A')
                resource_name = next((tag['Value'] for tag in all_tags if tag['Key'] == 'ResourceName'), 'N/A')

                # Find tags for this resource
                other_tags = [tag for tag in all_tags if tag['Key'] not in ['DeploymentId', 'ResourceName']]                
                formatted_tags = ", ".join([f"{tag['Key']}={tag['Value']}" for tag in other_tags])

                # Add to HTML
                html += f"<tr><td>{resource_arn}</td><td>{resource_type}</td><td>{resource_name}</td><td>{deployment_id}</td><td>{formatted_tags}</td></tr>"

        html += "</table>"
        html += "</body></html>" if arns else "No resources found</body></html>"
        return html

    except Exception as e:
        print(f"Error: {str(e)}")
        print(resources)
        return f"<html><body><h2>Error processing resources</h2><p>{str(e)}</p></body></html>"


def get_json(resource_groups, tagging_client, resource_groups_name):

    all_resources = []

    try:
        # Fetch resources and their types
        group_resources = resource_groups.list_group_resources(GroupName=resource_groups_name, MaxResults=50)
        resources = group_resources.get('ResourceIdentifiers', [])
        
        # Fetch tags for the ARNs gathered
        arns = [res['ResourceArn'] for res in resources]
        if arns:
            tagging_info = tagging_client.get_resources(ResourceARNList=arns)
            
            # Process and display each resource
            for res in resources:
                resource_arn = res['ResourceArn']
                resource_type = res['ResourceType']

                all_tags = [item['Tags'] for item in tagging_info['ResourceTagMappingList'] if item['ResourceARN'] == resource_arn][0]

                deployment_id = next((tag['Value'] for tag in all_tags if tag['Key'] == 'DeploymentId'), 'N/A')
                resource_name = next((tag['Value'] for tag in all_tags if tag['Key'] == 'ResourceName'), 'N/A')

                # Find tags for this resource
                other_tags = [tag for tag in all_tags if tag['Key'] not in ['DeploymentId', 'ResourceName']]                
                formatted_tags = ", ".join([f"{tag['Key']}={tag['Value']}" for tag in other_tags])

                # Add to HTML
                all_resources.append({
                    'resource_arn': resource_arn,
                    'resource_type': resource_type,
                    'resource_name': resource_name,
                    'deployment_id': deployment_id,
                    'other_tags': formatted_tags
                })

        return all_resources

    except Exception as e:
        print(f"Error: {str(e)}")
        print(resources)
        return json.dumps({
            'statusCode': 400,
            'body': f'Error: {str(e)}'
        })



def handler(event, context):
    resource_groups = boto3.client('resource-groups')
    tagging_client = boto3.client('resourcegroupstaggingapi')
    resource_groups_name = event.get('resource_groups_name')

    format = event.get('format')

    if format == 'html':
        return return_html_table(resource_groups, tagging_client, resource_groups_name)
    elif format == 'json':
        return json.dumps(get_json(resource_groups, tagging_client, resource_groups_name))
    else:
        return {
            'statusCode': 400,
            'body': json.dumps(f'Invalid format ({format})')
        }
