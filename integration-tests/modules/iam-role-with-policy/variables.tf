variable "role_name" {
  type        = string
  description = "Name of the IAM role"
}

variable "assume_role_policy" {
  type        = string
  description = "Assume role policy JSON"
}

variable "inline_policy" {
  type        = string
  description = "Inline policy JSON"
}
