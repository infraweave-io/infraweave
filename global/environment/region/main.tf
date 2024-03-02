
module "dashboard" {
  source = "./dashboard"

  name = "all-${var.environment}"
  resource_gather_function_arn = var.resource_gather_function_arn

  environment = var.environment
  region = var.region

  tag_filters = [
      {
        Key    = "Environment"
        Values = [var.environment]
      },
      {
        Key    = "Region"
        Values = [var.region]
      },
      {
        Key    = "DeploymentMethod"
        Values = ["InfraBridge"]
      }
    ]

}

module "dev_projects" {
  for_each = var.repositories
  source = "./project"

  module_name = each.value.name
  environment  = var.environment
  region = var.region
  clone_url_http = each.value.clone_url_http
  s3_module_bucket_full_path = "${var.buckets[var.region]}/${resource.aws_s3_object.empty_file.id}"
  resource_gather_function_arn = var.resource_gather_function_arn
  bucket_name = resource.aws_s3_bucket.terraform_state.bucket
  dynamodb_table_name = resource.aws_dynamodb_table.terraform_locks.name

}

resource "aws_dynamodb_table" "terraform_locks" {
#   name           = var.dynamodb_table_name
  name = "TerraformStateDynamoDBLocks-${var.region}-${var.environment}"
  billing_mode   = "PAY_PER_REQUEST"
  hash_key       = "LockID"

  attribute {
    name = "LockID"
    type = "S"
  }

  tags = {
    Name        = "TerraformStateLocks"
    # Environment = var.environment_tag
  }
}

resource "aws_s3_bucket" "terraform_state" {
#   bucket = var.bucket_name
  bucket_prefix = "tf-state-${var.region}-${var.environment}-"

  tags = {
    Name        = "TerraformStateBucket"
    # Environment = var.environment
    # Region      = var.region
  }
}

resource "aws_s3_bucket_versioning" "versioning_example" {
  bucket = aws_s3_bucket.terraform_state.id
  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_kms_key" "mykey" {
  description             = "This key is used to encrypt bucket objects"
  deletion_window_in_days = 10
}

resource "aws_s3_bucket_server_side_encryption_configuration" "example" {
  bucket = aws_s3_bucket.terraform_state.id

  rule {
    apply_server_side_encryption_by_default {
      kms_master_key_id = aws_kms_key.mykey.arn
      sse_algorithm     = "aws:kms"
    }
  }
}

resource "aws_s3_object" "empty_file" {
  key                    = "DO_NOT_REMOVE"
  bucket                 = var.buckets[var.region]
  content =               "this has to exist"
}