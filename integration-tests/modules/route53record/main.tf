terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

provider "aws" {
  region = "us-west-2"

  default_tags {
    tags = local.tags
  }
}

locals {
  tags = {
    Name        = "example.com"
    Environment = "dev"
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
