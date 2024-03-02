
variable "environment" {
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

variable "buckets" {
  type = map(string)
}

variable "region" {
  type = string
  
}