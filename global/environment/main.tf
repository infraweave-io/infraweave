

module "regional_resources_eu_central_1" {
  source = "./region"

  region = var.region
  environment = var.environment
  account_id = var.account_id
  modules = var.modules
  resource_gather_function_arn = module.resource_explorer.resource_gather_function_arn
  repositories = module.main_repositories.repositories
  buckets = var.buckets
  dynamodb_event_table_name = resource.aws_dynamodb_table.events.name
  dynamodb_deployment_table_name = resource.aws_dynamodb_table.deployments.name

}


module "main_repositories" {
  source = "./repo" 

  modules = var.modules
}

module "resource_explorer" {
  source = "./resource_explorer"

}

module "infra_api" {
  source = "./api_infra"

  environment = var.environment
  region = var.region
  events_table_name = resource.aws_dynamodb_table.events.name
  modules_table_name = resource.aws_dynamodb_table.modules.name
}

module "status_api" {
  source = "./api_status"

  environment = var.environment
  region = var.region
  events_table_name = resource.aws_dynamodb_table.events.name
}

module "deployment_api" {
  source = "./api_deployment"

  environment = var.environment
  region = var.region
  deployments_table_name = resource.aws_dynamodb_table.deployments.name
}

module "module_api" {
  source = "./api_module"

  environment = var.environment
  region = var.region
  modules_table_name = resource.aws_dynamodb_table.modules.name
  environments_table_name = resource.aws_dynamodb_table.environments.name
}

module "statistics_api" {
  source = "./api_statistics"

  environment = var.environment
  region = var.region
  events_table_name = resource.aws_dynamodb_table.events.name
}

module "ddb_stream_processor" {
  source = "./ddb_stream_processor"

  region = var.region
  sns_topic_arn = aws_sns_topic.events_topic.arn
}

resource "aws_sns_topic" "events_topic" {
  name = "events-topic-${var.region}-${var.environment}"
}

resource "aws_lambda_event_source_mapping" "ddb_to_lambda" {
  event_source_arn  = aws_dynamodb_table.events.stream_arn
  function_name     = module.ddb_stream_processor.ddb_stream_processor_arn
  starting_position = "LATEST"
}

resource "aws_dynamodb_table" "events" {
  name           = "Events-${var.region}-${var.environment}"
  billing_mode   = "PAY_PER_REQUEST"
  hash_key       = "deployment_id"
  range_key      = "epoch"

  stream_enabled   = true
  stream_view_type = "NEW_AND_OLD_IMAGES"

  attribute {
    name = "deployment_id"
    type = "S"
  }

  attribute {
    name = "epoch"
    type = "N"
  }

  attribute {
    name = "status"
    type = "S"
  }

  global_secondary_index {
    name               = "StatusIndex"
    hash_key           = "status"
    range_key          = "epoch"
    projection_type    = "ALL"
  }

  # ttl {
  #   attribute_name = "TimeToLive" # Define a TTL attribute if we want automatic expiration
  #   enabled        = false        # Set to true to enable TTL
  # }
  
  # lifecycle {
  #   # ignore_changes = [attribute_names]
  #   prevent_destroy = true
  # }

  tags = {
    Name = "EventsTable"
    # Environment = var.environment_tag
  }
}


resource "aws_dynamodb_table" "modules" {
  name           = "Modules-${var.region}-${var.environment}"
  billing_mode   = "PAY_PER_REQUEST"
  hash_key       = "module"
  range_key      = "environment_version"

  stream_enabled   = true
  stream_view_type = "NEW_AND_OLD_IMAGES"

  attribute {
    name = "module"
    type = "S"
  }

  attribute {
    name = "environment_version"
    type = "S"
  }

  attribute {
    name = "environment"
    type = "S"
  }

  attribute {
    name = "version"
    type = "S"
  }

  global_secondary_index {
    name               = "VersionEnvironmentIndex"
    hash_key           = "module"
    range_key          = "version"
    projection_type    = "ALL"
  }

  global_secondary_index {
    name               = "ModuleEnvironmentIndex"
    hash_key           = "module"
    range_key          = "environment"
    projection_type    = "ALL"
  }

  global_secondary_index {
    name               = "EnvironmentModuleVersionIndex"
    hash_key           = "environment"
    range_key          = "environment_version"
    projection_type    = "ALL"
  }

  # ttl {
  #   attribute_name = "TimeToLive" # Define a TTL attribute if we want automatic expiration
  #   enabled        = false        # Set to true to enable TTL
  # }
  
  # lifecycle {
  #   # ignore_changes = [attribute_names]
  #   prevent_destroy = true
  # }

  tags = {
    Name = "ModulesTable"
    # Environment = var.environment_tag
  }
}


resource "aws_dynamodb_table" "environments" {
  name           = "Environments-${var.region}-${var.environment}"
  billing_mode   = "PAY_PER_REQUEST"
  hash_key       = "environment"
  range_key      = "last_activity_epoch"

  attribute {
    name = "environment"
    type = "S"
  }

  attribute {
    name = "last_activity_epoch"
    type = "N"
  }

  tags = {
    Name = "EnvironmentsTable"
    # Environment = var.environment_tag
  }
}


resource "aws_dynamodb_table" "deployments" {
  name           = "Deployments-${var.region}-${var.environment}"
  billing_mode   = "PAY_PER_REQUEST"
  hash_key       = "deployment_id"

  attribute {
    name = "deployment_id"
    type = "S"
  }

  tags = {
    Name = "DeploymentsTable"
    # Environment = var.environment_tag
  }
}

# resource "aws_config_configuration_recorder" "config_recorder" {
#   name     = "config-recorder"
#   role_arn = aws_iam_role.config.arn

#   recording_group {
#     all_supported                 = true
#     include_global_resource_types = true
#   }
# }

# resource "aws_iam_role" "config" {
#   name = "aws-config-role-${var.region}-${var.environment}"

#   assume_role_policy = jsonencode({
#     Version = "2012-10-17",
#     Statement = [
#       {
#         Action = "sts:AssumeRole",
#         Principal = {
#           Service = "config.amazonaws.com",
#         },
#         Effect = "Allow",
#         Sid    = "",
#       },
#     ],
#   })
# }

# resource "aws_iam_policy" "config_access" {
#   name        = "ConfigAccessPolicy"
#   description = "Policy granting AWS Config access to resources."

#   policy = jsonencode({
#     Version = "2012-10-17",
#     Statement = [
#       {
#         Action = [
#           "s3:*",
#           "ec2:*",
#           "iam:*",
#           // Include additional actions as needed for other services
#         ],
#         Effect   = "Allow",
#         Resource = "*"
#       },
#     ]
#   })
# }

# resource "aws_iam_role_policy_attachment" "config_access_attachment" {
#   role       = aws_iam_role.config.name
#   policy_arn = aws_iam_policy.config_access.arn
# }


# resource "aws_config_delivery_channel" "config_channel" {
#   name            = "config-channel"
#   sns_topic_arn   = aws_sns_topic.config_notifications.arn
#   s3_bucket_name = aws_s3_bucket.config_bucket.id
#   snapshot_delivery_properties {
#     delivery_frequency = "One_Hour"
#   }

#   depends_on      = [aws_config_configuration_recorder.config_recorder]
# }

# resource "aws_sns_topic" "config_notifications" {
#   name = "config-notifications"
# }

# module "config_processor" {
#   source = "./config_processor"

#   region = var.region
#   dynamodb_event_table_name = resource.aws_dynamodb_table.events.name
# }

# resource "aws_s3_bucket" "config_bucket" {
#   bucket_prefix = "config-bucket-${var.region}-${var.environment}"

#   tags = {
#     Name        = "ConfigBucket"
#     Environment = var.environment
#   }
# }

# resource "aws_s3_bucket_policy" "config_bucket_policy" {
#   bucket = aws_s3_bucket.config_bucket.id

#   policy = jsonencode({
#     Version = "2012-10-17"
#     Statement = [
#       {
#         Action = [
#           "s3:GetBucketAcl",
#           "s3:GetBucketPolicy",
#           "s3:PutObject",
#           "s3:PutObjectAcl"
#         ]
#         Effect = "Allow"
#         Resource = [
#           "${aws_s3_bucket.config_bucket.arn}",
#           "${aws_s3_bucket.config_bucket.arn}/*"
#         ]
#         Principal = {
#           Service = "config.amazonaws.com"
#         }
#       }
#     ]
#   })
# }

# resource "null_resource" "start_config_recorder" {
#   depends_on = [aws_config_delivery_channel.config_channel]

#   provisioner "local-exec" {
#     command = "aws configservice start-configuration-recorder --configuration-recorder-name config-recorder"
#   }
# }

