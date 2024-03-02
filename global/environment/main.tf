

module "resource_explorer" {
  for_each = var.regions
  source = "./region"

  region = each.value
  environment = var.environment
  account_id = var.account_id
  modules = var.modules

  providers = {
    aws = aws
  }
}
