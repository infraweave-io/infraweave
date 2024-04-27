
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
  name          = "${var.module_name}-${var.region}-${var.environment}"
  description   = "InfraBridge worker for region ${var.region}"
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
    type             = "CODECOMMIT"
    location         = var.clone_url_http
    buildspec        = <<-EOT
      version: 0.2

      phases:
        install:
          commands:
            - apt-get update && apt-get install -y wget unzip jq bc
            - wget https://releases.hashicorp.com/terraform/1.5.7/terraform_1.5.7_linux_amd64.zip
            - unzip terraform_1.5.7_linux_amd64.zip
            - mv terraform /usr/local/bin/
        pre_build:
          commands:
            # - terraform fmt -check
            - export LOG_QUEUE_NAME=logs-$${DEPLOYMENT_ID}
            # - aws sqs create-queue --queue-name $LOG_QUEUE_NAME # Due to delay in queue creation, we will create the queue in the apiInfra lambda
            - echo "Started work..." >> terraform_init_output.txt
            - aws sqs send-message --queue-url "https://sqs.$${REGION}.amazonaws.com/$${ACCOUNT_ID}/$LOG_QUEUE_NAME" --message-body "$(cat terraform_init_output.txt)" &
            - >
              while sleep 1; do
                if pgrep terraform > /dev/null; then
                  aws sqs send-message --queue-url "https://sqs.$${REGION}.amazonaws.com/$${ACCOUNT_ID}/$LOG_QUEUE_NAME" --message-body "$(cat terraform_init_output.txt)" &
                else
                  break
                fi
              done &
            - terraform init -no-color -backend-config="bucket=$${TF_BUCKET}" -backend-config="key=$${ENVIRONMENT}/$${REGION}/$${DEPLOYMENT_ID}/terraform.tfstate" -backend-config="region=$${REGION}" -backend-config="dynamodb_table=$${TF_DYNAMODB_TABLE}" | tee terraform_init_output.txt
            - ret=$?
            - echo "\n\n\n\nInitiated with return code $ret" >> terraform_init_output.txt
            - terraform validate
            - aws sqs send-message --queue-url https://sqs.$${REGION}.amazonaws.com/$${ACCOUNT_ID}/$LOG_QUEUE_NAME --message-body "$(cat terraform_init_output.txt)"
        build:
          commands:
            - export LOG_QUEUE_NAME=logs-$${DEPLOYMENT_ID}
            - export STATUS="started"
            - epoch_seconds=$(date +%s) # Seconds since epoch
            - nanoseconds=$(date +%N) # Nanoseconds since last second
            - epoch_milliseconds=$(echo "$epoch_seconds * 1000 + $nanoseconds / 1000000" | bc) # Convert nanoseconds to milliseconds and concatenate
            - >
              echo $${SIGNAL} | jq --arg status "$STATUS" --arg epoch "$epoch_milliseconds" --arg ts "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" ". + {status: \$status, timestamp: \$ts, epoch: (\$epoch | tonumber), id: (.deployment_id + \"-\" + .module + \"-\" + .name + \"-\" + .event + \"-\" + \$epoch + \"-\" + \$status)}" > signal_ts_id.json
            - >
              jq 'with_entries(if .value | type == "string" then .value |= {"S": .} elif .value | type == "number" then .value |= {"N": (tostring)} elif .value | type == "object" then .value |= {"M": (with_entries(if .value | type == "string" then .value |= {"S": .} else . end))} else . end)' signal_ts_id.json > signal_dynamodb.json
            - aws dynamodb put-item --table-name $${DYNAMODB_EVENT_TABLE} --item file://signal_dynamodb.json
            - echo "Started terraform $${EVENT}..." >> terraform_output.txt
            - aws sqs send-message --queue-url "https://sqs.$${REGION}.amazonaws.com/$${ACCOUNT_ID}/$LOG_QUEUE_NAME" --message-body "$(cat terraform_output.txt)" &
            - >
              while sleep 1; do
                if pgrep terraform > /dev/null; then
                  aws sqs send-message --queue-url "https://sqs.$${REGION}.amazonaws.com/$${ACCOUNT_ID}/$LOG_QUEUE_NAME" --message-body "$(cat terraform_output.txt)" &
                else
                  break
                fi
              done &
            - terraform $${EVENT} -auto-approve -no-color -var "environment=$${ENVIRONMENT}" -var "region=$${REGION}" -var "module_name=$${MODULE_NAME}" -var "deployment_id=$${DEPLOYMENT_ID}" > terraform_output.txt 2>&1 && export ret=0 || export ret=$?
            - echo "\n\n\n\nFinished with return code $ret" >> terraform_output.txt
            - cat terraform_output.txt
            - export INPUT_VARIABLES="$(printenv | grep '^TF_VAR_' | sed 's/^TF_VAR_//;s/=/":"/;s/^/{"/;s/$/\"}/' | jq -s 'add')"
            - aws sqs send-message --queue-url https://sqs.$${REGION}.amazonaws.com/053475148537/$LOG_QUEUE_NAME --message-body "$(cat terraform_output.txt)"
            - >
              echo "{\"deployment_id\":\"$${DEPLOYMENT_ID}\", \"input_variables\": $INPUT_VARIABLES, \"epoch\": $epoch_milliseconds, \"environment\": \"$${ENVIRONMENT}\", \"module\": \"$${MODULE_NAME}\"}" > deployment.json
            - cat deployment.json
            - >
              if [ "$EVENT" = "destroy" -a $ret -eq 0 ]; then
                jq '. += {"deleted": 1}' deployment.json > deployment_full.json
              elif [ $ret -eq 0 ]; then
                jq '. += {"deleted": 0}' deployment.json > deployment_full.json
              else
                jq '' deployment.json > deployment_full.json
              fi
            - >
              jq 'with_entries(if .value | type == "string" then .value |= {"S": .} elif .value | type == "number" then .value |= {"N": (tostring)} elif .value | type == "object" then .value |= {"M": (with_entries(if .value | type == "string" then .value |= {"S": .} else . end))} else . end)' deployment_full.json > deployment_dynamodb.json
            - cat deployment_dynamodb.json
            - >
              if [ $ret -eq 0 ]; then
                aws dynamodb put-item --table-name $${DYNAMODB_DEPLOYMENT_TABLE} --item file://deployment_dynamodb.json
              fi
        post_build:
          commands:
            - awk '/Terraform used the /{p=1}p' terraform_output.txt > tf.txt
            - export STATUS="finished"
            - echo $${SIGNAL}
            - tail -10000 tf.txt
            - epoch_seconds=$(date +%s) # Seconds since epoch
            - nanoseconds=$(date +%N) # Nanoseconds since last second
            - epoch_milliseconds=$(echo "$epoch_seconds * 1000 + $nanoseconds / 1000000" | bc) # Convert nanoseconds to milliseconds and concatenate
            - >
              echo $${SIGNAL} | jq --arg status "$STATUS" --arg tfContent "$(cat tf.txt)" --arg epoch "$epoch_milliseconds" --arg ts "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" '. + {status: $status, metadata: {terraform: $tfContent}, timestamp: $ts, epoch: ($epoch | tonumber), id: (.deployment_id + "-" + .module + "-" + .name + "-" + .event + "-" + $epoch + "-" + $status)}' > signal_ts_id.json
            - >
              jq 'with_entries(if .value | type == "string" then .value |= {"S": .} elif .value | type == "number" then .value |= {"N": (tostring)} elif .value | type == "object" then .value |= {"M": (with_entries(if .value | type == "string" then .value |= {"S": .} else . end))} else . end)' signal_ts_id.json > signal_dynamodb.json
            - aws dynamodb put-item --table-name $${DYNAMODB_EVENT_TABLE} --item file://signal_dynamodb.json
      EOT
  }
}

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