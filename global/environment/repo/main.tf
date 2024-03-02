
resource "aws_codecommit_repository" "module" {
  for_each = var.modules

  repository_name = "${each.value.name}"
  description     = "A module repository created with Terraform for ${each.value.repo}"
  
}