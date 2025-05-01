terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

provider "aws" {
  default_tags {
    tags = merge(
      var.tags,
      {
        INFRAWEAVE_REFERENCE = var.INFRAWEAVE_REFERENCE
      }
    )
  }
}

resource "aws_s3_bucket" "example" {
  bucket = var.bucket_name
}
