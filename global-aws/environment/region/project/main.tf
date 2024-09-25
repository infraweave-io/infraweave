
resource "aws_iam_role" "codebuild_service_role" {
  name = "codebuild-${var.module_name}-${var.region}-${var.environment}-service-role"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Action = "sts:AssumeRole"
        Principal = {
          Service = "ecs-tasks.amazonaws.com"
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
          "*",
          "lambda:InvokeFunction",
        ]
        Resource = "*" # Replace with your specific resources
      },
    ]
  })
}

data "aws_caller_identity" "current" {}

# resource "aws_codebuild_project" "terraform_apply" {
#   name         = "${var.module_name}-${var.region}-${var.environment}"
#   description  = "InfraBridge worker for region ${var.region}"
#   service_role = aws_iam_role.codebuild_service_role.arn

#   artifacts {
#     type = "NO_ARTIFACTS"
#   }

#   #   cache {
#   #     type     = "S3"
#   #     location = "your-s3-bucket-for-caching" # Replace with your S3 bucket name
#   #   }

#   environment {
#     compute_type                = "BUILD_GENERAL1_SMALL"
#     image                       = "aws/codebuild/standard:5.0" # Build your own based on this: https://github.com/aws/aws-codebuild-docker-images/tree/master/ubuntu/standard
#     type                        = "LINUX_CONTAINER"
#     image_pull_credentials_type = "CODEBUILD"

#     environment_variable {
#       name  = "ACCOUNT_ID"
#       value = data.aws_caller_identity.current.account_id
#     }

#     environment_variable {
#       name  = "TF_BUCKET"
#       value = var.tf_bucket_name
#     }
#     environment_variable {
#       name  = "TF_DYNAMODB_TABLE"
#       value = var.tf_dynamodb_table_name
#     }
#     environment_variable {
#       name  = "DYNAMODB_DEPLOYMENT_TABLE"
#       value = var.dynamodb_deployment_table_name
#     }
#     environment_variable {
#       name  = "DYNAMODB_EVENT_TABLE"
#       value = var.dynamodb_event_table_name
#     }
#     environment_variable {
#       name  = "ENVIRONMENT"
#       value = var.environment
#     }
#     environment_variable {
#       name  = "REGION"
#       value = var.region
#     }
#     environment_variable {
#       name  = "MODULE_NAME"
#       value = var.module_name
#     }
#     environment_variable {
#       name  = "SIGNAL"
#       value = "OVERRIDE-ME"
#     }
#     environment_variable {
#       name  = "DEPLOYMENT_ID"
#       value = "OVERRIDE-ME"
#     }
#     environment_variable {
#       name  = "EVENT"
#       value = "OVERRIDE-ME"
#     }
#     dynamic "environment_variable" {
#       for_each = var.terraform_environment_variables
#       content {
#         name  = environment_variable.key
#         value = environment_variable.value
#       }
#     }
#   }

#   source_version = var.environment

#   source {
#     type      = "CODECOMMIT"
#     location  = var.clone_url_http
#     buildspec = file("${path.module}/buildspec.yml")
#   }
# }

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

# VPC and Networking Setup
resource "aws_vpc" "main" {
  cidr_block = "10.0.0.0/16"
}

resource "aws_subnet" "public" {
  count                   = 2
  vpc_id                  = aws_vpc.main.id
  cidr_block              = cidrsubnet(aws_vpc.main.cidr_block, 8, count.index)
  availability_zone       = element(["${var.region}a", "${var.region}b"], count.index)
  map_public_ip_on_launch = true
}

resource "aws_ssm_parameter" "ecs_subnet_id" {
  name  = "/infrabridge/${var.region}/${var.environment}/ecs_subnet_id"
  type  = "String"
  value = resource.aws_subnet.public[0].id # TODO: use both subnets
}

resource "aws_internet_gateway" "gw" {
  vpc_id = aws_vpc.main.id
}

resource "aws_route_table" "public" {
  vpc_id = aws_vpc.main.id
  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.gw.id
  }
}

resource "aws_route_table_association" "public" {
  count          = 2
  subnet_id      = aws_subnet.public[count.index].id
  route_table_id = aws_route_table.public.id
}

# Security Group
resource "aws_security_group" "ecs_sg" {
  vpc_id = aws_vpc.main.id

  #   ingress {
  #     from_port   = 80
  #     to_port     = 80
  #     protocol    = "tcp"
  #     cidr_blocks = ["0.0.0.0/0"]
  #   }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

resource "aws_ssm_parameter" "ecs_security_group" {
  name  = "/infrabridge/${var.region}/${var.environment}/ecs_security_group"
  type  = "String"
  value = resource.aws_security_group.ecs_sg.id
}

# ECS Cluster
resource "aws_ecs_cluster" "ecs_cluster" {
  name = "terraform-ecs-cluster"
}

resource "aws_ssm_parameter" "ecs_cluster_name" {
  name  = "/infrabridge/${var.region}/${var.environment}/ecs_cluster_name"
  type  = "String"
  value = resource.aws_ecs_cluster.ecs_cluster.name
}

# IAM Role for ECS Task Execution
resource "aws_iam_role" "ecs_task_execution_role" {
  name = "ecsTaskExecutionRole"

  assume_role_policy = jsonencode({
    Version = "2012-10-17",
    Statement = [{
      Action = "sts:AssumeRole",
      Effect = "Allow",
      Principal = {
        Service = "ecs-tasks.amazonaws.com"
      }
    }]
  })
}

# Attach necessary policies to the IAM Role
resource "aws_iam_role_policy_attachment" "ecs_task_execution_role_policy" {
  role       = aws_iam_role.ecs_task_execution_role.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}

# ECS Task Definition for running Terraform
resource "aws_ecs_task_definition" "terraform_task" {
  family                   = "terraform-task"
  network_mode             = "awsvpc"
  requires_compatibilities = ["FARGATE"]
  cpu                      = "1024"
  memory                   = "2048"
  execution_role_arn       = aws_iam_role.ecs_task_execution_role.arn
  task_role_arn            = aws_iam_role.codebuild_service_role.arn

  runtime_platform {
    cpu_architecture        = "ARM64"
    operating_system_family = "LINUX"
  }

  container_definitions = jsonencode([{
    name      = "terraform-docker"
    image     = "053475148537.dkr.ecr.eu-central-1.amazonaws.com/terraform-docker-ecs-test:latest"
    cpu       = 1024
    memory    = 2048
    essential = true
    # command   = ["version"] # Example command for testing
    logConfiguration = {
      logDriver = "awslogs"
      options = {
        awslogs-group         = "/ecs/terraform"
        awslogs-region        = var.region
        awslogs-stream-prefix = "ecs"
      }
    }
    environment = [
      {
        name  = "ACCOUNT_ID"
        value = data.aws_caller_identity.current.account_id
      },
      {
        name  = "TF_BUCKET"
        value = var.tf_bucket_name
      },
      {
        name  = "TF_DYNAMODB_TABLE"
        value = var.tf_dynamodb_table_name
      },
      {
        name  = "DYNAMODB_DEPLOYMENT_TABLE"
        value = var.dynamodb_deployment_table_name
      },
      {
        name  = "DYNAMODB_EVENT_TABLE"
        value = var.dynamodb_event_table_name
      },
      {
        name  = "ENVIRONMENT"
        value = var.environment
      },
      {
        name  = "REGION"
        value = var.region
      },
      {
        name  = "MODULE_NAME"
        value = var.module_name
      },
      # {
      #   name  = "SIGNAL"
      #   value = "OVERRIDE-ME"
      # },
      # {
      #   name  = "DEPLOYMENT_ID"
      #   value = "OVERRIDE-ME"
      # },
      # {
      #   name  = "EVENT"
      #   value = "OVERRIDE-ME"
      # }
    ]
  }])
}

resource "aws_ssm_parameter" "ecs_task_definition" {
  name  = "/infrabridge/${var.region}/${var.environment}/ecs_task_definition"
  type  = "String"
  value = resource.aws_ecs_task_definition.terraform_task.family
}

resource "aws_cloudwatch_log_group" "ecs_log_group" {
  name              = "/ecs/terraform"
  retention_in_days = 7 # Optional retention period
}
