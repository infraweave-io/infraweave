# INFRAWEAVE variables - These are set automatically when being deployed using InfraWeave

variable "INFRAWEAVE_DEPLOYMENT_ID" {
  type    = string
  default = "N/A"
}

variable "INFRAWEAVE_ENVIRONMENT" {
  type    = string
  default = "N/A"
}

variable "INFRAWEAVE_REFERENCE" {
  type    = string
  default = "N/A"
}

variable "INFRAWEAVE_MODULE_VERSION" {
  type    = string
  default = "N/A"
}

variable "INFRAWEAVE_MODULE_TYPE" {
  type    = string
  default = "N/A"
}

variable "INFRAWEAVE_MODULE_TRACK" {
  type    = string
  default = "N/A"
}

variable "INFRAWEAVE_DRIFT_DETECTION" {
  type    = string
  default = "N/A"
}

variable "INFRAWEAVE_DRIFT_DETECTION_INTERVAL" {
  type    = string
  default = "N/A"
}

# INFRAWEAVE GIT variables
variable "INFRAWEAVE_GIT_COMMITTER_EMAIL" {
  type    = string
  default = "N/A"
}

variable "INFRAWEAVE_GIT_COMMITTER_NAME" {
  type    = string
  default = "N/A"
}

variable "INFRAWEAVE_GIT_ACTOR_USERNAME" {
  type    = string
  default = "N/A"
}

variable "INFRAWEAVE_GIT_ACTOR_PROFILE_URL" {
  type    = string
  default = "N/A"
}

variable "INFRAWEAVE_GIT_REPOSITORY_NAME" {
  type    = string
  default = "N/A"
}

variable "INFRAWEAVE_GIT_REPOSITORY_PATH" {
  type    = string
  default = "N/A"
}

variable "INFRAWEAVE_GIT_COMMIT_SHA" {
  type    = string
  default = "N/A"
}
