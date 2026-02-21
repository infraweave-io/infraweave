resource "local_file" "counted" {
  count    = 2
  filename = "${path.module}/count_${count.index}.txt"
  content  = "count content ${count.index}"
}

variable "files" {
  default = {
    a = "content_a"
    b = "content_b"
  }
}

resource "local_file" "foreach" {
  for_each = var.files
  filename = "${path.module}/foreach_${each.key}.txt"
  content  = each.value
}

resource "local_file" "dependent" {
  filename = "${path.module}/dependent.txt"
  content  = "dependent content"
  depends_on = [
    local_file.counted,
    local_file.foreach
  ]
}
