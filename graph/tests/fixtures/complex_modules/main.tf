variable "environment" {
  default = "dev"
}

module "network" {
  source = "./modules/network"
  vpc_cidr = "10.0.0.0/16"
}

module "app" {
  source = "./modules/app"
  vpc_id = module.network.vpc_id
  subnet_id = module.network.subnet_id
  env = var.environment
}

resource "local_file" "root_config" {
  content = "app_ip: ${module.app.app_ip}"
  filename = "${path.module}/config.txt"
}

output "final_config_path" {
  value = local_file.root_config.filename
}