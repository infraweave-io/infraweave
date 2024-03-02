

module "regional_resources_eu_central_1" {
  source = "./region"

  region = "eu-central-1"
  environment = var.environment
  account_id = var.account_id
  modules = var.modules
  resource_gather_function_arn = module.resource_explorer.resource_gather_function_arn
  repositories = module.main_repositories.repositories
  buckets = var.buckets

  providers = {
    aws = aws.eu-central-1
  }
}

module "regional_resources_eu_west_1" {
  source = "./region"

  region = "eu-west-1"
  environment = var.environment
  account_id = var.account_id
  modules = var.modules
  resource_gather_function_arn = module.resource_explorer.resource_gather_function_arn
  repositories = module.main_repositories.repositories
  buckets = var.buckets

  providers = {
    aws = aws.eu-west-1
  }
}

module "regional_resources_us_east_1" {
  source = "./region"

  region = "us-east-1"
  environment = var.environment
  account_id = var.account_id
  modules = var.modules
  resource_gather_function_arn = module.resource_explorer.resource_gather_function_arn
  repositories = module.main_repositories.repositories
  buckets = var.buckets

  providers = {
    aws = aws.us-east-1
  }
}

module "main_repositories" {
  source = "./repo" 

  modules = var.modules
}

module "resource_explorer" {

  source = "./resource_explorer"

  providers = {
    aws = aws.eu-central-1
  }
}
