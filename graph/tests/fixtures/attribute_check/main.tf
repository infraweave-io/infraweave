resource "local_file" "base" {
  filename = "base.txt"
  content  = "base content"
}

resource "local_file" "dependent" {
  filename = "dependent.txt"
  content  = local_file.base.content
}

resource "local_file" "dependent_multi" {
  filename = "dependent_multi.txt"
  content  = "${local_file.base.content}-${local_file.base.filename}"
}
