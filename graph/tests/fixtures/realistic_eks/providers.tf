terraform {
  required_providers {
    aws = {
      source = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

provider "aws" {
  default_tags {
    tags = local.tags
  }
}

locals {
  tags = merge(var.tags, { INFRAWEAVE_DEPLOYMENT_ID = var.INFRAWEAVE_DEPLOYMENT_ID, INFRAWEAVE_ENVIRONMENT = var.INFRAWEAVE_ENVIRONMENT, INFRAWEAVE_REFERENCE = var.INFRAWEAVE_REFERENCE, INFRAWEAVE_MODULE_VERSION = var.INFRAWEAVE_MODULE_VERSION, INFRAWEAVE_MODULE_TYPE = var.INFRAWEAVE_MODULE_TYPE, INFRAWEAVE_MODULE_TRACK = var.INFRAWEAVE_MODULE_TRACK, INFRAWEAVE_DRIFT_DETECTION = var.INFRAWEAVE_DRIFT_DETECTION, INFRAWEAVE_DRIFT_DETECTION_INTERVAL = var.INFRAWEAVE_DRIFT_DETECTION_INTERVAL, INFRAWEAVE_GIT_COMMITTER_EMAIL = var.INFRAWEAVE_GIT_COMMITTER_EMAIL, INFRAWEAVE_GIT_COMMITTER_NAME = var.INFRAWEAVE_GIT_COMMITTER_NAME, INFRAWEAVE_GIT_ACTOR_USERNAME = var.INFRAWEAVE_GIT_ACTOR_USERNAME, INFRAWEAVE_GIT_ACTOR_PROFILE_URL = var.INFRAWEAVE_GIT_ACTOR_PROFILE_URL, INFRAWEAVE_GIT_REPOSITORY_NAME = var.INFRAWEAVE_GIT_REPOSITORY_NAME, INFRAWEAVE_GIT_REPOSITORY_PATH = var.INFRAWEAVE_GIT_REPOSITORY_PATH, INFRAWEAVE_GIT_COMMIT_SHA = var.INFRAWEAVE_GIT_COMMIT_SHA })
}

variable "tags" {
  type = map(string)
  description = "Any other tags you might want to set"
  default = {}
}

variable "INFRAWEAVE_DEPLOYMENT_ID" {
  type = string
  default = "N/A"
}

variable "INFRAWEAVE_ENVIRONMENT" {
  type = string
  default = "N/A"
}

variable "INFRAWEAVE_REFERENCE" {
  type = string
  default = "N/A"
}

variable "INFRAWEAVE_MODULE_VERSION" {
  type = string
  default = "N/A"
}

variable "INFRAWEAVE_MODULE_TYPE" {
  type = string
  default = "N/A"
}

variable "INFRAWEAVE_MODULE_TRACK" {
  type = string
  default = "N/A"
}

variable "INFRAWEAVE_DRIFT_DETECTION" {
  type = string
  default = "N/A"
}

variable "INFRAWEAVE_DRIFT_DETECTION_INTERVAL" {
  type = string
  default = "N/A"
}

variable "INFRAWEAVE_GIT_COMMITTER_EMAIL" {
  type = string
  default = "N/A"
}

variable "INFRAWEAVE_GIT_COMMITTER_NAME" {
  type = string
  default = "N/A"
}

variable "INFRAWEAVE_GIT_ACTOR_USERNAME" {
  type = string
  default = "N/A"
}

variable "INFRAWEAVE_GIT_ACTOR_PROFILE_URL" {
  type = string
  default = "N/A"
}

variable "INFRAWEAVE_GIT_REPOSITORY_NAME" {
  type = string
  default = "N/A"
}

variable "INFRAWEAVE_GIT_REPOSITORY_PATH" {
  type = string
  default = "N/A"
}

variable "INFRAWEAVE_GIT_COMMIT_SHA" {
  type = string
  default = "N/A"
}

variable "cluster_name" {
  type = string
  description = "Name of the EKS cluster to be created"
  nullable = false

  validation {
    condition = can(regex("^[a-zA-Z0-9][a-zA-Z0-9._-]*[a-zA-Z0-9]$", var.cluster_name)) && length(var.cluster_name) >= 1 && length(var.cluster_name) <= 100
    error_message = "Cluster name must be 1-100 characters, start and end with alphanumeric characters, and can contain letters, numbers, hyphens, underscores, and periods."
  }
}

variable "cluster_version" {
  type = string
  description = "Kubernetes version for the EKS cluster"
  nullable = false
  default = "1.33"

  validation {
    condition = contains(["1.31", "1.32", "1.33"], var.cluster_version)
    error_message = "cluster_version must be one of: 1.31, 1.32, or 1.33."
  }
}

variable "vpc_id" {
  type = string
  description = "ID of VPC to use"
}

variable "subnet_ids" {
  type = list(string)
  description = "List of subnet IDs to use for the EKS cluster"
  default = null
  nullable = true
}

output "kubernetes_endpoint" {
  value = module.eks.kubernetes_endpoint
}

output "kubernetes_certificate_authority_data" {
  value = module.eks.kubernetes_certificate_authority_data
}

output "cluster_name" {
  value = module.eks.cluster_name
}
