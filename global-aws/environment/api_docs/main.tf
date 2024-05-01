
resource "aws_ecr_repository" "docs_lambda_repository" {
  name                 = "docs-lambda-container-repository"
  image_tag_mutability = "IMMUTABLE"
  force_delete = true
}

resource "aws_iam_role" "lambda_execution_role" {
  name = "lambda_docs_execution_role-${var.region}"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Action = "sts:AssumeRole"
        Effect = "Allow"
        Principal = {
          Service = "lambda.amazonaws.com"
        }
      },
    ]
  })
}

resource "aws_iam_policy" "lambda_policy" {
  name        = "lambda_docs_access_policy-${var.region}"
  description = "IAM policy for Lambda to access necessary AWS services"

  policy = data.aws_iam_policy_document.lambda_policy_document.json
}

data "aws_iam_policy_document" "lambda_policy_document" {
  statement {
    actions = [
      "dynamodb:*",
      "logs:CreateLogGroup",
      "logs:CreateLogStream",
      "logs:PutLogEvents",
      "ecr:GetAuthorizationToken",
      "ecr:BatchCheckLayerAvailability",
      "ecr:GetDownloadUrlForLayer",
      "ecr:BatchGetImage"
    ]
    resources = ["*"]
  }
  statement {
    actions = [
      "s3:PutObject",
    ]
    resources = [aws_s3_bucket.docs_bucket.arn, "${aws_s3_bucket.docs_bucket.arn}/*"]
  }
}

resource "aws_iam_role_policy_attachment" "lambda_policy_attachment" {
  role       = aws_iam_role.lambda_execution_role.name
  policy_arn = aws_iam_policy.lambda_policy.arn
}

resource "aws_lambda_function" "api_docs" {
  function_name = "DocsDeploymentStatusApi"
  package_type  = "Image"
  image_uri     = "${aws_ecr_repository.docs_lambda_repository.repository_url}:47"

  timeout = 155
  role    = aws_iam_role.lambda_execution_role.arn

  architectures = [ "arm64" ]

  memory_size = 512

  environment {
    variables = {
      DYNAMODB_MODULES_TABLE_NAME = var.modules_table_name
      REGION                          = var.region
      ENVIRONMENT                     = var.environment
      DOCS_BUCKET                     = aws_s3_bucket.docs_bucket.bucket
    }
  }

  depends_on = [ aws_ecr_repository.docs_lambda_repository ]
}

resource "aws_s3_bucket" "docs_bucket" {
  bucket_prefix = "infrabridge-docs-"
}