output "kubernetes_endpoint" {
  value=module.eks.cluster_endpoint
}

output "kubernetes_certificate_authority_data" {
  value=base64decode(module.eks.cluster_certificate_authority_data)
}

output "cluster_name" {
  value=module.eks.cluster_name
}