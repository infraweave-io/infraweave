variable "modules_table_name" {
    type    = string
}

variable "modules_s3_bucket" {
    type    = string
}

variable "environments_table_name" {
    type = string
}

variable "region" {
    type    = string
}

variable "environment" {
    type    = string
}

variable "docs_generator_function_arn" {
    type    = string
}