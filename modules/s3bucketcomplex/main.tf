# main.tf

terraform {
  # backend "s3" {
  # }
}

# Specify the AWS provider
provider "aws" {
  region = "us-east-1"
}

# Include the random provider for generating unique bucket names
provider "random" {}

# Generate a random string to ensure unique bucket names
resource "random_string" "bucket_suffix" {
  length  = 8
  special = false
  upper   = false
}

# Complex variable: Configuration settings for resources
variable "configurations" {
  type = object({
    s3_bucket = object({
      bucket_prefix      = string
      versioning_enabled = bool
    })
    dynamodb_table = object({
      table_name   = string
      hash_key     = string
      billing_mode = string
      attributes = list(object({
        name = string
        type = string
      }))
    })
  })

  default = {
    s3_bucket = {
      bucket_prefix      = "my-unique-bucket"
      versioning_enabled = false
    }
    dynamodb_table = {
      table_name   = "my-dynamodb-table"
      hash_key     = "id"
      billing_mode = "PAY_PER_REQUEST"
      attributes = [
        {
          name = "id"
          type = "S"
        }
      ]
    }
  }
}

# Create the S3 bucket using the complex variable and random string
resource "aws_s3_bucket" "bucket" {
  bucket = "${var.configurations.s3_bucket.bucket_prefix}-${random_string.bucket_suffix.result}"

  versioning {
    enabled = var.configurations.s3_bucket.versioning_enabled
  }
}

# Create the DynamoDB table using the complex variable
resource "aws_dynamodb_table" "table" {
  name         = var.configurations.dynamodb_table.table_name
  hash_key     = var.configurations.dynamodb_table.hash_key
  billing_mode = var.configurations.dynamodb_table.billing_mode

  dynamic "attribute" {
    for_each = var.configurations.dynamodb_table.attributes
    content {
      name = attribute.value.name
      type = attribute.value.type
    }
  }
}

# Outputs for verification
output "s3_bucket_name" {
  value = aws_s3_bucket.bucket.id
}

output "dynamodb_table_name" {
  value = aws_dynamodb_table.table.name
}
