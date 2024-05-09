
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
        Sid    = ""
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
          "kms:DescribeKey",
          "sqs:sendmessage",
        ]
        Resource = "*" # Replace with your specific resources
      },
    ]
  })
}

data "aws_caller_identity" "current" {}

resource "aws_codebuild_project" "terraform_apply" {
  name         = "${var.module_name}-${var.region}-${var.environment}"
  description  = "InfraBridge worker for region ${var.region}"
  service_role = aws_iam_role.codebuild_service_role.arn

  artifacts {
    type = "NO_ARTIFACTS"
  }

  #   cache {
  #     type     = "S3"
  #     location = "your-s3-bucket-for-caching" # Replace with your S3 bucket name
  #   }

  environment {
    compute_type                = "BUILD_GENERAL1_SMALL"
    image                       = "aws/codebuild/standard:5.0" # Build your own based on this: https://github.com/aws/aws-codebuild-docker-images/tree/master/ubuntu/standard
    type                        = "LINUX_CONTAINER"
    image_pull_credentials_type = "CODEBUILD"

    environment_variable {
      name  = "ACCOUNT_ID"
      value = data.aws_caller_identity.current.account_id
    }

    environment_variable {
      name  = "TF_BUCKET"
      value = var.tf_bucket_name
    }
    environment_variable {
      name  = "TF_DYNAMODB_TABLE"
      value = var.tf_dynamodb_table_name
    }
    environment_variable {
      name  = "DYNAMODB_DEPLOYMENT_TABLE"
      value = var.dynamodb_deployment_table_name
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
      name  = "DEPLOYMENT_ID"
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
    type      = "CODECOMMIT"
    location  = var.clone_url_http
    buildspec = file("${path.module}/buildspec.yml")
  }
}

module "dashboard" {
  source = "../dashboard"

  name                         = "${var.module_name}-${var.region}-${var.environment}"
  resource_gather_function_arn = var.resource_gather_function_arn

  environment = var.environment
  region      = var.region

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
