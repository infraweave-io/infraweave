terraform {
  required_version = "~> 1.5"

  required_providers {
    kubernetes = {
      source  = "hashicorp/kubernetes"
      version = "~> 2.0"
    }
  }
}

provider "kubernetes" {
  host                   = local.kubernetes_endpoint
  cluster_ca_certificate = local.kubernetes_ca_certificate
  token                  = local.kubernetes_token
}