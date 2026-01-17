module "eks" {
  source = "./EKS-0.1.2-dev"
  cluster_name = var.cluster_name
  cluster_version = var.cluster_version
  vpc_id = var.vpc_id
  subnet_ids = var.subnet_ids
  providers = {
    aws = aws
  }
}
