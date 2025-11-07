terraform {
  required_providers {
    helm = {
      source  = "hashicorp/helm"
      version = "~>3.0"
    }
  }
}

provider "helm" {
  kubernetes = {
    host                   = local.kubernetes_endpoint
    cluster_ca_certificate = local.kubernetes_ca_certificate
    token                  = local.kubernetes_token
  }
}
