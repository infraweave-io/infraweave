

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
  source = "./infra_api"

  environment = var.environment
  region = var.region
  events_table_name = resource.aws_dynamodb_table.events.name
}


resource "aws_dynamodb_table" "events" {
  name = "Events-${var.region}-${var.environment}"
  billing_mode   = "PAY_PER_REQUEST"

  hash_key       = "id"

  attribute {
    name = "id"
    type = "S"
  }

  # TODO Add GCS indices here if needed

  # lifecycle {
  #   # ignore_changes = [attribute_names]
  #   prevent_destroy = true
  # }

  tags = {
    Name        = "EventsTable"
    # Environment = var.environment_tag
  }
}