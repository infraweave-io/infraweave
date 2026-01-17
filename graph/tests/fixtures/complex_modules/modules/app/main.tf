variable "vpc_id" {}
variable "subnet_id" {}
variable "env" {}

module "db" {
    source = "./modules/db"
}

resource "local_file" "app_server" {
    content = "app running in ${var.vpc_id} subnet ${var.subnet_id} using db ${module.db.db_endpoint} in ${var.env}"
    filename = "${path.module}/app.txt"
}

output "app_ip" {
    value = "1.2.3.4"
    depends_on = [local_file.app_server]
}