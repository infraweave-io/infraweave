resource "local_file" "inner" {
  content  = "inner"
  filename = "${path.module}/inner.txt"
}

output "inner_id" {
  value = local_file.inner.id
}