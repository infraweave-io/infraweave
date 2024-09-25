
variable "module_name" {
  type = string
}

variable "environment" {
  type = string
}

variable "region" {
  type = string
}

variable "clone_url_http" {
  type        = string
  description = "The clone URL of the CodeCommit repository"
}

variable "terraform_environment_variables" {
  description = "Map of environment variables for the CodeBuild project"
  type        = map(string)
  default = {
    # TF_VAR_example_variable = "example_value"
    # ENVIRONMENT             = "dev"
    # REGION = "eu-central-1"

    # Add more variables here
  }
}

variable "resource_gather_function_arn" {
  type = string
}

variable "tf_bucket_name" {
  type = string
}

variable "tf_dynamodb_table_name" {
  type = string
}

variable "dynamodb_deployment_table_name" {
  type = string
}

variable "dynamodb_event_table_name" {
  type = string
}
