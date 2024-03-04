

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
}

module "status_api" {
  source = "./api_status"

  environment = var.environment
  region = var.region
  events_table_name = resource.aws_dynamodb_table.events.name
}

module "statistics_api" {
  source = "./api_statistics"

  environment = var.environment
  region = var.region
  events_table_name = resource.aws_dynamodb_table.events.name
}

resource "aws_dynamodb_table" "events" {
  name           = "Events-${var.region}-${var.environment}"
  billing_mode   = "PAY_PER_REQUEST"
  hash_key       = "deployment_id"
  range_key      = "epoch"

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

