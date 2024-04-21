resource "null_resource" "lambda_package" {
  triggers = {
    lambda_file_hash = filesha256("${path.module}/lambda.py")
  }

  provisioner "local-exec" {
    command = <<-EOT
      echo "Resolved path: ${path.module}" && \
      rm -rf package && \
      mkdir package && \
      python3 -m pip install -r ${path.module}/requirements.txt --target package && \
      cp ${path.module}/lambda.py package/ && \
      cp ${path.module}/schema_module.yaml package/ && \
      cd package && \
      zip -r9 ../${path.module}/lambda_function_payload.zip .
    EOT
  }
}

resource "aws_lambda_function" "api_module" {
  function_name = "moduleApi"
  runtime       = "python3.10"
  handler       = "lambda.handler"
  architectures = [ "arm64" ]

  timeout = 10

  filename      = "${path.module}/lambda_function_payload.zip"
  role          = aws_iam_role.iam_for_lambda.arn

  source_code_hash = filebase64sha256("${path.module}/lambda.py")

  environment {
    variables = {
      DYNAMODB_MODULES_TABLE_NAME = var.modules_table_name
      DYNAMODB_ENVIRONMENTS_TABLE_NAME = var.environments_table_name
      MODULE_S3_BUCKET = var.modules_s3_bucket
      REGION              = var.region
      ENVIRONMENT         = var.environment
    }
  }

  depends_on = [null_resource.lambda_package]
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
      "dynamodb:*",
      "logs:CreateLogGroup",
      "logs:CreateLogStream",
      "logs:PutLogEvents",
      "s3:GetObject",
      "s3:PutObject",
    ]
    resources = ["*"] # TODO: This should be limited to the specific resources that the Lambda function needs access to
  }
}

resource "aws_iam_role" "iam_for_lambda" {
  name               = "lambda_module_api_role"
  assume_role_policy = data.aws_iam_policy_document.assume_role.json
}

resource "aws_iam_policy" "lambda_policy" {
  name        = "lambda_module_access_policy"
  description = "IAM policy for Lambda to read module from the event database and access CloudWatch Logs"
  policy      = data.aws_iam_policy_document.lambda_policy_document.json
}

resource "aws_iam_role_policy_attachment" "lambda_policy_attachment" {
  role       = aws_iam_role.iam_for_lambda.name
  policy_arn = aws_iam_policy.lambda_policy.arn
}

# data "archive_file" "lambda" {
#   type        = "zip"
#   source_file = "${path.module}/lambda.py"
#   output_path = "${path.module}/lambda_function_payload.zip"
# }
