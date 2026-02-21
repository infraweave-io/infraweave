resource "local_file" "foo" {
  content  = "foo!"
  filename = "${path.module}/foo.txt"
}

output "foo_id" {
  value = local_file.foo.id
}

output "foo_content" {
  value = local_file.foo.content
}