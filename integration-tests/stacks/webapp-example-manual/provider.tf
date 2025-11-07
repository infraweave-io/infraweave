provider "aws" {
  default_tags {
    tags = merge(local.tags, { INFRAWEAVE_REFERENCE = var.INFRAWEAVE_REFERENCE })
  }
  use_fips_endpoint = true
}