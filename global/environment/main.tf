

module "regional_resources_eu_central_1" {
  source = "./region"

  region = var.region
  environment = var.environment
  account_id = var.account_id
  modules = var.modules
  resource_gather_function_arn = module.resource_explorer.resource_gather_function_arn
  repositories = module.main_repositories.repositories
  buckets = var.buckets

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

}
