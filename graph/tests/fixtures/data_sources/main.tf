data "local_file" "read" {
  filename = "${path.module}/preexisting.txt"
}

resource "local_file" "destination" {
  content  = data.local_file.read.content
  filename = "${path.module}/dest.txt"
}