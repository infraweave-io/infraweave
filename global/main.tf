
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
    }
    # prod = "123456789012"
  }
  buckets = {
    eu-central-1 = "tf-modules-bucket-482njk4krnw"
    eu-west-1 = "tf-modules-bucket-9nfkjsdnkf"
    us-east-1 = "tf-modules-bucket-jljowijr32z"
  }

}

module "account_resources" {
  for_each = local.accounts
  source = "./environment"

  environment = each.value.environment
  account_id = each.value.account_id
  modules = local.modules
  buckets = local.buckets
  region = "eu-central-1"

}
