
variable "environment" {
  type    = string
}

variable "regions" {
  type    = set(string)
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