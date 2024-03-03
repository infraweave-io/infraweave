
resource "aws_iam_role" "codebuild_service_role" {
  name = "codebuild-${var.module_name}-${var.region}-${var.environment}-service-role"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Action = "sts:AssumeRole"
        Principal = {
          Service = "codebuild.amazonaws.com"
        }
        Effect = "Allow"
        Sid = ""
      },
    ]
  })
}

resource "aws_iam_role_policy" "codebuild_policy" {
  name = "codebuild-${var.module_name}-${var.environment}-policy"
  role = aws_iam_role.codebuild_service_role.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = [
          "logs:CreateLogGroup",
          "logs:CreateLogStream",
          "logs:PutLogEvents",
          "codecommit:GitPull",
          "s3:*",
          "dynamodb:PutItem",
          "dynamodb:GetItem",
          "dynamodb:DeleteItem",
          "kms:Encrypt",
          "kms:Decrypt",
          "kms:ReEncrypt*",
          "kms:GenerateDataKey*",
          "kms:DescribeKey"
        ]
        Resource = "*" # Replace with your specific resources
      },
    ]
  })
}
resource "aws_codebuild_project" "terraform_apply" {
  name          = "terraform-${var.module_name}-${var.region}-${var.environment}"
  description   = "Runs terraform apply on a specific module"
  service_role  = aws_iam_role.codebuild_service_role.arn

  artifacts {
    type = "NO_ARTIFACTS"
  }

#   cache {
#     type     = "S3"
#     location = "your-s3-bucket-for-caching" # Replace with your S3 bucket name
#   }

  environment {
    compute_type                = "BUILD_GENERAL1_SMALL"
    image                       = "aws/codebuild/standard:5.0" # Use an image with Terraform installed or install it in buildspec
    type                        = "LINUX_CONTAINER"
    image_pull_credentials_type = "CODEBUILD"

    environment_variable {
      name  = "TF_BUCKET"
      value = var.tf_bucket_name
    }
    environment_variable {
      name  = "TF_DYNAMODB_TABLE"
      value = var.tf_dynamodb_table_name
    }
    environment_variable {
      name  = "DYNAMODB_EVENT_TABLE"
      value = var.dynamodb_event_table_name
    }
    environment_variable {
      name  = "ENVIRONMENT"
      value = var.environment
    }
    environment_variable {
      name  = "REGION"
      value = var.region
    }
    environment_variable {
      name  = "MODULE_NAME"
      value = var.module_name
    }
    environment_variable {
      name  = "SIGNAL"
      value = "OVERRIDE-ME"
    }
    environment_variable {
      name  = "ID"
      value = "OVERRIDE-ME"
    }
    environment_variable {
      name  = "EVENT"
      value = "OVERRIDE-ME"
    }
    dynamic "environment_variable" {
      for_each = var.terraform_environment_variables
      content {
        name  = environment_variable.key
        value = environment_variable.value
      }
    }
  }

  source_version = var.environment

  source {
    type             = "CODECOMMIT"
    location         = var.clone_url_http
    buildspec        = <<-EOT
      version: 0.2

      phases:
        install:
          commands:
            - apt-get update && apt-get install -y wget unzip jq
            - wget https://releases.hashicorp.com/terraform/1.5.7/terraform_1.5.7_linux_amd64.zip
            - unzip terraform_1.5.7_linux_amd64.zip
            - mv terraform /usr/local/bin/
            - terraform init -backend-config="bucket=$${TF_BUCKET}" -backend-config="key=$${ENVIRONMENT}/$${REGION}/$${ID}/terraform.tfstate" -backend-config="region=$${REGION}" -backend-config="dynamodb_table=$${TF_DYNAMODB_TABLE}"
        pre_build:
          commands:
            # - terraform fmt -check
            - terraform validate
            - export STATUS="started"
            - >
              echo $${SIGNAL} | jq --arg status "$STATUS" --arg epoch "$(date -u +%s)" --arg ts "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" ". + {status: \$status, timestamp: \$ts, id: (.deployment_id + \"-\" + .module + \"-\" + .name + \"-\" + .event + \"-\" + \$epoch + \"-\" + \$status)}" > signal_ts_id.json
            - >
              jq 'with_entries(if .value | type == "string" then .value |= {"S": .} elif .value | type == "object" then .value |= {"M": (with_entries(if .value | type == "string" then .value |= {"S": .} else . end))} else . end)' signal_ts_id.json > signal_dynamodb.json
            - aws dynamodb put-item --table-name $${DYNAMODB_EVENT_TABLE} --item file://signal_dynamodb.json
        build:
          commands:
            - echo "building..."
            - terraform $${EVENT} -auto-approve -no-color -var "environment=$${ENVIRONMENT}" -var "region=$${REGION}" -var "module_name=$${MODULE_NAME}" -var "deployment_id=$${ID}" | tee terraform_output.txt
        post_build:
          commands:
            - awk '/Terraform used the /{p=1}p' terraform_output.txt > tf.txt
            - export STATUS="finished"
            - echo $${SIGNAL}
            - tail -10 tf.txt
            - >
              echo $${SIGNAL} | jq --arg status "$STATUS" --arg tfContent "$(cat tf.txt)" --arg epoch "$(date -u +%s)" --arg ts "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" '. + {status: $status, metadata: {terraform: $tfContent}, timestamp: $ts, id: (.deployment_id + "-" + .module + "-" + .name + "-" + .event + "-" + $epoch + "-" + $status)}' > signal_ts_id.json
            - >
              jq 'with_entries(if .value | type == "string" then .value |= {"S": .} elif .value | type == "object" then .value |= {"M": (with_entries(if .value | type == "string" then .value |= {"S": .} else . end))} else . end)' signal_ts_id.json > signal_dynamodb.json
            - aws dynamodb put-item --table-name $${DYNAMODB_EVENT_TABLE} --item file://signal_dynamodb.json
      EOT
  }
}

# resource "aws_resourcegroups_group" "owner_marius_group" {
#   name = "resources-${var.module_name}-${var.environment}"

#   resource_query {
#     query = jsonencode({
#       ResourceTypeFilters = ["AWS::AllSupported"]
#       TagFilters          = [
#         {
#           Key    = "Environment"
#           Values = [var.environment]
#         },
#         {
#           Key    = "ModuleName"
#           Values = [var.module_name]
#         },
#         {
#           Key    = "Region"
#           Values = [var.region]
#         },
#         {
#           Key    = "DeploymentMethod"
#           Values = ["InfraBridge"]
#         }
#       ]
#     })
#   }
# }

# resource "aws_cloudwatch_dashboard" "example_dashboard" {
#   dashboard_name = "Dashboard-${var.module_name}-${var.environment}"

#   dashboard_body = <<EOF
# {
#   "widgets": [
#     {
#       "type": "custom",
#       "x": 0,
#       "y": 0,
#       "width": 24,
#       "height": 10,
#       "properties": {
#         "title": "Resources Table",
#         "endpoint": "arn:aws:lambda:eu-central-1:053475148537:function:resourceGathererFunction",
#         "params": {
#             "resource_groups_name": "${aws_resourcegroups_group.owner_marius_group.name}"
#         },
#         "updateOn": {
#             "refresh": true
#         },
#         "title": "Resources Table"
#       }
#     }
#   ]
# }
# EOF
# }

module "dashboard" {
  source = "../dashboard"

  name = "${var.module_name}-${var.region}-${var.environment}"
  resource_gather_function_arn = var.resource_gather_function_arn

  environment = var.environment
  region = var.region

  tag_filters = [
      {
        Key    = "Environment"
        Values = [var.environment]
      },
      {
        Key    = "ModuleName"
        Values = [var.module_name]
      },
      {
        Key    = "Region"
        Values = [var.region]
      },
      {
        Key    = "DeploymentMethod"
        Values = ["InfraBridge"]
      }
    ]

}