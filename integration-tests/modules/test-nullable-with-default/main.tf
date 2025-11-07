terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

resource "aws_s3_bucket" "example" {
  bucket = var.another_var

  tags = {
    MyVar = var.my_var == null ? "was-null" : var.my_var
  }
}
