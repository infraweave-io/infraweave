
variable "bucket_name" {
  type    = string
  description = "Name of the S3 bucket. This must be globally unique and can contain only lowercase letters, numbers, hyphens, and periods. It must be between 3 and 63 characters long."
}

variable "enable_acl" {
  type     = bool
  default  = false
  nullable = false
  description = "Enable ACL for the S3 bucket. If set to true, the bucket will be created with a bucket policy that grants full control to the AWS account owner."
}

variable "tags" {
  type = map(string)
  default = {
    Test = "override-me"
  }
}

variable "INFRAWEAVE_REFERENCE" {
  type = string
  description = "This is to be set automatically during runtime"
  default = ""
}
