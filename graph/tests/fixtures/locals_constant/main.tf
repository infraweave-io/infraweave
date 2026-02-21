locals {
  constant_val = "foo-value"
}

resource "local_file" "f" {
  content  = local.constant_val
  filename = "${path.module}/file.txt"
}
