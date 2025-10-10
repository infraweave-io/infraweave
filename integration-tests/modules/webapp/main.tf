terraform {
  required_version = "~> 1.5"

  required_providers {
    kubernetes = {
      source  = "hashicorp/kubernetes"
      version = "~> 2.0"
    }
  }
}

resource "kubernetes_service" "this" {
  metadata {
    name = var.app_name
  }
  spec {
    selector = {
      app = kubernetes_deployment.this.metadata.0.labels.app
    }
    session_affinity = "ClientIP"
    port {
      port        = var.service_port
      target_port = var.app_port
    }

    type = "LoadBalancer"
  }
}

resource "kubernetes_ingress" "this" {
  wait_for_load_balancer = true
  metadata {
    name = var.app_name
    annotations = {
      "kubernetes.io/ingress.class" = "nginx"
    }
  }
  spec {
    rule {
      http {
        path {
          path = "/*"
          backend {
            service_name = kubernetes_service.this.metadata.0.name
            service_port = var.service_port
          }
        }
      }
    }
  }
}

resource "kubernetes_deployment" "this" {
  metadata {
    name = var.app_name
    labels = {
      app = var.app_name
    }
  }

  spec {
    replicas = 3

    selector {
      match_labels = {
        app = var.app_name
      }
    }

    template {
      metadata {
        labels = {
          app = var.app_name
        }
      }

      spec {
        container {
          image = var.app_image
          name  = var.app_name

          resources {
            limits = {
              cpu    = "0.5"
              memory = "512Mi"
            }
            requests = {
              cpu    = "250m"
              memory = "50Mi"
            }
          }

          port {
            container_port = var.app_port
          }

          liveness_probe {
            http_get {
              path = "/"
              port = var.app_port
            }

            initial_delay_seconds = 3
            period_seconds        = 3
          }
        }
      }
    }
  }
}