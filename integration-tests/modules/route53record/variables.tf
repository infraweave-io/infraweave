variable "records" {
  description = "A list of records"
  type        = list(string)
  default     = ["dev1.example.com", "dev2.example.com"]
}

variable "ttl" {
  description = "The TTL of the record"
  type        = number
}

variable "tags" {
  description = "A mapping of tags to assign to the resource"
  type        = map(string)
  default = {
    "Name"        = "example.com"
    "Environment" = "dev"
  }
}
