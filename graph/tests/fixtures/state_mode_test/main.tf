resource "local_file" "state_test" {
  content  = "this is state content"
  filename = "${path.module}/state_test.txt"
}

output "file_id" {
  value = local_file.state_test.id
}
