variable "cluster_name" {
  type        = string
  description = "Name of the EKS cluster to be created"
  nullable    = false
  validation {
    condition     = can(regex("^[a-zA-Z0-9][a-zA-Z0-9._-]*[a-zA-Z0-9]$", var.cluster_name)) && length(var.cluster_name) >= 1 && length(var.cluster_name) <= 100
    error_message = "Cluster name must be 1-100 characters, start and end with alphanumeric characters, and can contain letters, numbers, hyphens, underscores, and periods."
  }
}

variable "cluster_version" {
  type        = string
  description = "Kubernetes version for the EKS cluster"
  nullable    = false
  default     = "1.33"
  validation {
    condition     = contains(["1.31", "1.32", "1.33"], var.cluster_version)
    error_message = "cluster_version must be one of: 1.31, 1.32, or 1.33."
  }
}


variable "vpc_id" {
  type        = string
  description = "ID of VPC to use"
}

variable "subnet_ids" {
  type        = list(string)
  description = "List of subnet IDs to use for the EKS cluster"
  default     = null
  nullable    = true
}
