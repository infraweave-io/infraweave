module "inner" {
  source = "./inner"
}

output "root_out" {
  value = module.inner.inner_out
}
