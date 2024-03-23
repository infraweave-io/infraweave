variable "devops_job_pat" {
  type = string
  description = "Value of the Azure DevOps PAT in order to be able to trigger the job from the api"
  nullable = false
}

variable "region" {
  type = string
  description = "The region where the resources will be deployed"
  nullable = false
  default = "East US"
}

variable "environment" {
  type = string
  description = "The environment where the resources will be deployed"
  nullable = false
  default = "dev"
}
