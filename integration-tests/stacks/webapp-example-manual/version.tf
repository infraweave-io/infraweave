terraform {
  required_version = "~> 1.5"

  required_providers {
    aws = {
      source = "hashicorp/aws"
      version = "5.96.0"
    }
    kubernetes = {
      source = "hashicorp/kubernetes"
      version = "2.36.0"
    }
  }
}