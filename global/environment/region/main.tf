
resource "aws_codecommit_repository" "module" {
  for_each = var.modules

  repository_name = "${each.value.name}" # Name your repository
  description     = "A module repository created with Terraform for ${each.value.repo}"
  
}

module "dashboard" {
  source = "./dashboard"

  name = "all-${var.environment}"
  resource_gather_function_arn = module.resource_explorer.resource_gather_function_arn

  tag_filters = [
      {
        Key    = "Environment"
        Values = [var.environment]
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

  providers = {
    aws = aws
  }
}

module "dev_projects" {
  for_each = aws_codecommit_repository.module
  source = "./project"

  module_name = each.value.repository_name
  environment  = "dev"
  region = "eu-central-1"
  clone_url_http = each.value.clone_url_http
  resource_gather_function_arn = module.resource_explorer.resource_gather_function_arn

  providers = {
    aws = aws
  }
}

module "resource_explorer" {
  source = "./resource_explorer"

  providers = {
    aws = aws
  }
}
