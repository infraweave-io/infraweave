module "my_child" {
  source = "./child"
}

resource "local_file" "outer" {
    content = module.my_child.inner_id
    filename = "${path.module}/outer.txt"
}