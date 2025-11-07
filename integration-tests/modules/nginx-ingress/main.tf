terraform {
  required_providers {
    helm = {
      source  = "hashicorp/helm"
      version = ">=3.0"
    }
  }
}

resource "helm_release" "nginx_ingress" {
  name       = "nginx-ingress-controller"
  repository = "https://charts.bitnami.com/bitnami"
  chart      = "nginx-ingress-controller"

  set = [
    {
      name  = "service.type"
      value = var.service_type
    }
  ]
}
