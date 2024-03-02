
variable "name" {
  type    = string
}

variable "tag_filters" {
    type    = list
}

variable "resource_gather_function_arn" {
  type    = string
}

variable "environment" {
  type = string
}

variable "region" {
  type = string  
}
