variable "nullable_with_default" {
  type     = string
  default  = null
  nullable = true
  description = "This is a nullable variable with a default value = null"
}

variable "nullable_without_default" {
  type     = string
  nullable = true
  description = "This is a nullable variable with a default value = null"
}
