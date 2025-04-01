locals {
  tags = {
    Name        = "example.com"
    Environment = "dev"
  }
}

provider "aws" {
  region = "us-west-2"

  default_tags {
    tags = local.tags
  }
}

# Hardcoded some values just for this test
resource "aws_route53_record" "example" {
  zone_id = "Z123456ABCDEF"
  name    = "example.com"
  type    = "A"
  ttl     = var.ttl
  records = var.records
}
