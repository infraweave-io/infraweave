
variable "environment" {
  type    = string
}

variable "region" {
  type    = string
}

variable "account_id" {
  type    = string
}

variable "modules" {
  type    = map(object({
    name = string
    repo = string
  }))
}

variable "resource_gather_function_arn" {
  type = string
}

variable "repositories" {
  type = map(object({
    name = string
    clone_url_http = string
  }))
  
}

variable "buckets" {
  type = map(string)
}

variable "dynamodb_event_table_name" {
  type = string
}

variable "dynamodb_deployment_table_name" {
  type    = string
}