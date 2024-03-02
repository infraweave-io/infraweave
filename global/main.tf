
locals {
  modules = {
    s3bucket = {
      name = "s3bucket"
      repo = "https://github.com/proformance/terraform-module-test"
    }
    iamrole = {
      name = "iamrole"
      repo = "https://github.com/proformance/terraform-module-test"
    }
  }
  accounts = {
    test_dev = {
        environment = "dev",
        account_id = "123456789012",
        regions = [
            "eu-central-1"
        ]
    }
    # prod = "123456789012"
  }

}

module "resource_explorer" {
  for_each = local.accounts
  source = "./environment"

  environment = each.value.environment
  regions = each.value.regions
  account_id = each.value.account_id
  modules = local.modules

  providers = {
    aws = aws
  }
}
