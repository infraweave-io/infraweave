variable "my_var" {
  type     = string
  default  = "standard"
  nullable = true
  description = "This is a nullable variable with a default value of 'standard', but can be set to null"
}

variable "another_var" {
  type     = string
  nullable = false
  description = "A required non-nullable variable"
}
