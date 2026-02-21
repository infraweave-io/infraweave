variable "input" {
    default = "hello"
}

locals {
    intermediate = var.input
    derived = "${local.intermediate}-world"
}

resource "local_file" "out" {
    content = local.derived
    filename = "${path.module}/out.txt"
}