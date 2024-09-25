
resource "aws_lambda_function" "api" {
  function_name = "infrabridge_api"
  runtime       = "python3.12"
  handler       = "lambda.handler"

  timeout = 15

  filename = "${path.module}/lambda_function_payload.zip"
  role     = aws_iam_role.iam_for_lambda.arn

  source_code_hash = filebase64sha256("${path.module}/lambda_function_payload.zip")

  environment {
    variables = {
      DYNAMODB_EVENTS_TABLE_NAME      = var.events_table_name
      DYNAMODB_MODULES_TABLE_NAME     = var.modules_table_name
      DYNAMODB_DEPLOYMENTS_TABLE_NAME = var.deployments_table_name
      MODULE_S3_BUCKET                = var.modules_s3_bucket
      REGION                          = var.region
      ENVIRONMENT                     = var.environment
      ECS_CLUSTER_NAME                = "terraform-ecs-cluster"
      ECS_TASK_DEFINITION             = "terraform-task"
      SUBNET_ID                       = "subnet-0e1c2ac5ce4f2e767"
      SECURITY_GROUP_ID               = "sg-067b7d80fcb63057e"
    }
  }
}

data "aws_iam_policy_document" "assume_role" {
  statement {
    effect = "Allow"

    principals {
      type        = "Service"
      identifiers = ["lambda.amazonaws.com"]
    }

    actions = ["sts:AssumeRole"]
  }
}

data "aws_iam_policy_document" "lambda_policy_document" {
  statement {
    actions = [
      "ecs:RunTask",
      "iam:PassRole",
      "dynamodb:PutItem",
      "dynamodb:Query",
      "logs:CreateLogGroup",
      "logs:CreateLogStream",
      "logs:PutLogEvents",
      "logs:GetLogEvents",
      "sqs:createqueue",
      "s3:GetObject", # for pre-signed URLs
      "s3:PutObject", # to upload modules
    ]
    resources = ["*"]
  }
}

resource "aws_iam_role" "iam_for_lambda" {
  name               = "infrabridge_api_role-${var.region}"
  assume_role_policy = data.aws_iam_policy_document.assume_role.json
}

resource "aws_iam_policy" "lambda_policy" {
  name        = "infrabridge_api_access_policy-${var.region}"
  description = "IAM policy for Lambda to launch CodeBuild and access CloudWatch Logs"
  policy      = data.aws_iam_policy_document.lambda_policy_document.json
}

resource "aws_iam_role_policy_attachment" "lambda_policy_attachment" {
  role       = aws_iam_role.iam_for_lambda.name
  policy_arn = aws_iam_policy.lambda_policy.arn
}

data "archive_file" "lambda" {
  type        = "zip"
  source_file = "${path.module}/lambda.py"
  output_path = "${path.module}/lambda_function_payload.zip"
}
