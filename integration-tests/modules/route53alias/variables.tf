variable "domain_name" {
  description = "Domain name to use for zone and record"
  default = "example.com"
  type    = string
}

variable "alias_name" {
  description = "DNS domain name for a CloudFront distribution, S3 bucket, ELB, AWS Global Accelerator, or another resource record set in this hosted zone."
  type = string
}

variable "alias_zone_id" {
  description = "Hosted zone ID for a CloudFront distribution, S3 bucket, ELB, AWS Global Accelerator, or Route 53 hosted zone."
  type = string
}

variable "alias_evalute_target_health" {
  description = "Set to true if you want Route 53 to determine whether to respond to DNS queries using this resource record set by checking the health of the resource record set."
  type = bool
  default = false
}