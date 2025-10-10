output "kubernetes_endpoint" {
  value=module.eks.cluster_endpoint
}

output "kubernetes_certificate_authority_data" {
  value=base64decode(module.eks.cluster_certificate_authority_data)
}

output "kubernetes_token" {
  value=data.aws_eks_cluster_auth.cluster.token
}