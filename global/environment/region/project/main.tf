
resource "aws_iam_role" "codebuild_service_role" {
  name = "codebuild-${var.module_name}-${var.environment}-service-role"

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
  name          = "terraform-${var.module_name}-${var.environment}"
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
      name  = "BUCKET"
      value = resource.aws_s3_bucket.terraform_state.bucket
    }
    environment_variable {
      name  = "DYNAMODB_TABLE"
      value = resource.aws_dynamodb_table.terraform_locks.name
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
      name  = "ID"
      value = "OVERRIDE-ME"
    }
    dynamic "environment_variable" {
      for_each = var.terraform_environment_variables
      content {
        name  = environment_variable.key
        value = environment_variable.value
      }
    }
    environment_variable {
      name  = "INPUT_VARIABLES_JSON"
      value = "{}"
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
            - echo $${INPUT_VARIABLES_JSON} > input_variables.json
            - cat input_variables.json
            - apt-get update && apt-get install -y wget unzip
            - wget https://releases.hashicorp.com/terraform/1.1.0/terraform_1.1.0_linux_amd64.zip
            - unzip terraform_1.1.0_linux_amd64.zip
            - mv terraform /usr/local/bin/
            - terraform init -backend-config="bucket=$${BUCKET}" -backend-config="key=$${ENVIRONMENT}/$${REGION}/$${ID}/terraform.tfstate" -backend-config="region=$${REGION}" -backend-config="dynamodb_table=$${DYNAMODB_TABLE}"
        pre_build:
          commands:
            # - terraform fmt -check
            - terraform validate
        build:
          commands:
            - terraform apply -auto-approve -var-file="input_variables.json" -var "environment=$${ENVIRONMENT}" -var "region=$${REGION}" -var "module_name=$${MODULE_NAME}" -var "terraform_id=$${ID}"
      EOT
  }
}

resource "aws_s3_bucket" "terraform_state" {
#   bucket = var.bucket_name
  bucket = "terraform-state-bucket-${var.module_name}-${var.environment}"

  tags = {
    Name        = "TerraformStateBucket"
    # Environment = var.environment_tag
  }
}

resource "aws_dynamodb_table" "terraform_locks" {
#   name           = var.dynamodb_table_name
  name = "TerraformStateDynamoDBLocks-${var.module_name}-${var.environment}"
  billing_mode   = "PAY_PER_REQUEST"
  hash_key       = "LockID"

  attribute {
    name = "LockID"
    type = "S"
  }

  tags = {
    Name        = "TerraformStateLocks"
    # Environment = var.environment_tag
  }
}

resource "aws_s3_bucket_versioning" "versioning_example" {
  bucket = aws_s3_bucket.terraform_state.id
  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_kms_key" "mykey" {
  description             = "This key is used to encrypt bucket objects"
  deletion_window_in_days = 10
}

resource "aws_s3_bucket_server_side_encryption_configuration" "example" {
  bucket = aws_s3_bucket.terraform_state.id

  rule {
    apply_server_side_encryption_by_default {
      kms_master_key_id = aws_kms_key.mykey.arn
      sse_algorithm     = "aws:kms"
    }
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

  name = "${var.module_name}-${var.environment}"
  resource_gather_function_arn = var.resource_gather_function_arn

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

  providers = {
    aws = aws
  }
}