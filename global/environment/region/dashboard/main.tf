
resource "aws_resourcegroups_group" "owner_marius_group" {
  name = "resources-${var.name}-${var.region}-${var.environment}"

  resource_query {
    query = jsonencode({
      ResourceTypeFilters = ["AWS::AllSupported"]
      TagFilters          = var.tag_filters
    })
  }
}

resource "aws_cloudwatch_dashboard" "example_dashboard" {
  dashboard_name = "Dashboard-${var.name}"

  dashboard_body = <<EOF
{
  "widgets": [
    {
      "type": "custom",
      "x": 0,
      "y": 0,
      "width": 24,
      "height": 10,
      "properties": {
        "title": "Resources Table",
        "endpoint": "${var.resource_gather_function_arn}",
        "params": {
            "resource_groups_name": "${aws_resourcegroups_group.owner_marius_group.name}"
        },
        "updateOn": {
            "refresh": true
        },
        "title": "Resources Table"
      }
    }
  ]
}
EOF
}