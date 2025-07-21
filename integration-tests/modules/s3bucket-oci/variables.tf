
variable "bucket_name" {
  nullable = false
  type    = string
  description = "Name of the S3 bucket. This must be globally unique and can contain only lowercase letters, numbers, hyphens, and periods. It must be between 3 and 63 characters long."
}

variable "enable_acl" {
  type     = bool
  default  = false
  nullable = false
  description = "Enable ACL for the S3 bucket. If set to true, the bucket will be created with a bucket policy that grants full control to the AWS account owner."
}

variable "unused_variable_without_default" {
  type = string
  nullable = true
}

variable "unused_variable_with_default" {
  type = string
  nullable = true
  default = null
}

variable "tags" {
  type = map(string)
  default = {
    Test = "override-me"
  }
}


######## IMPLICIT VARIABLES

variable "INFRAWEAVE_GIT_COMMITTER_EMAIL" {
  type    = string
  default = ""
}

variable "INFRAWEAVE_GIT_COMMITTER_NAME" {
  type    = string
  default = ""
}

variable "INFRAWEAVE_GIT_ACTOR_USERNAME" {
  type    = string
  default = ""
}

variable "INFRAWEAVE_GIT_ACTOR_PROFILE_URL" {
  type    = string
  default = ""
}

variable "INFRAWEAVE_GIT_REPOSITORY_NAME" {
  type    = string
  default = ""
}

variable "INFRAWEAVE_GIT_REPOSITORY_PATH" {
  type    = string
  default = ""
}

variable "INFRAWEAVE_DEPLOYMENT_ID" {
  type    = string
  default = ""
}

variable "INFRAWEAVE_ENVIRONMENT"  {
  type    = string
  default = ""
}

variable "INFRAWEAVE_REFERENCE"  {
  type    = string
  default = ""
}
