variable "domain_name" {
  description = "Domain name to use for zone and record"
  default = "example.com"
  type    = string
}

variable "records" {
  description = "A list of records"
  type        = list(string)
  default     = ["dev1.example.com", "dev2.example.com"]
}

variable "ttl" {
  description = "The TTL of the record"
  type        = number
}
