terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

module "eks" {
  source  = "terraform-aws-modules/eks/aws"
  version = "20.37.1"

  cluster_name    = var.cluster_name
  cluster_version = var.cluster_version

  cluster_compute_config = {
    enabled    = true
    node_pools = ["general-purpose"]
  }

  enable_auto_mode_custom_tags = true

  vpc_id     = var.vpc_id
  subnet_ids = var.subnet_ids

  providers = {
    aws=aws
  }
}