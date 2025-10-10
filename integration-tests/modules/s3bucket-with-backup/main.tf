terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
      configuration_aliases = [ aws.us-east-1 ]
    }
  }
}

resource "aws_s3_bucket" "primary" {
  bucket = var.bucket_name
}


resource "aws_s3_bucket" "backup" {
  bucket = "${var.bucket_name}-backup"
  provider = aws.us-east-1
}
