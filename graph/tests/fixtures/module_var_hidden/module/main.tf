variable "inner_val" {
  default = "default_value"
}

resource "local_file" "inner" {
  content  = var.inner_val
  filename = "${path.module}/inner.txt"
}
