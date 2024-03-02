import json
import boto3

def handler(event, context):
    resource_groups = boto3.client('resource-groups')
    tagging_client = boto3.client('resourcegroupstaggingapi')
    resource_groups_name = event.get('resource_groups_name')

    # HTML header
    html = f"<html><body><h2>Resource Group: {resource_groups_name}</h2><table border='1'>"
    html += "<tr><th>Resource ARN</th><th>Resource Type</th><th>Tags</th></tr>"

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

                # Find tags for this resource
                tags = next((item['Tags'] for item in tagging_info['ResourceTagMappingList'] if item['ResourceARN'] == resource_arn), {})
                formatted_tags = ", ".join([f"{tag['Key']}={tag['Value']}" for tag in tags])

                # Add to HTML
                html += f"<tr><td>{resource_arn}</td><td>{resource_type}</td><td>{formatted_tags}</td></tr>"

        html += "</table>"
        html += "</body></html>" if arns else "No resources found</body></html>"
        return html

    except Exception as e:
        print(f"Error: {str(e)}")
        return f"<html><body><h2>Error processing resources</h2><p>{str(e)}</p></body></html>"
