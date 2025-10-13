module "webapp" {
  source = "./WebApp-0.5.5-dev+test.1"
  app_image = var.webapp__app_image
  app_name = var.webapp__app_name
  app_port = var.webapp__app_port
  service_port = var.webapp__service_port
  providers = {
    kubernetes = kubernetes
  }
  depends_on = [
    module.eks, module.nginxingress
  ]
}