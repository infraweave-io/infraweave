variable "vpc_cidr" {}

resource "local_file" "vpc" {
  content = "vpc: ${var.vpc_cidr}"
  filename = "${path.module}/vpc.txt"
}

resource "local_file" "subnet" {
  content = "subnet: ${local_file.vpc.id}"
  filename = "${path.module}/subnet.txt"
}

output "vpc_id" {
  value = local_file.vpc.id
}

output "subnet_id" {
  value = local_file.subnet.id
}