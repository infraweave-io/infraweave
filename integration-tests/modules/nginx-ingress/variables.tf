
variable "service_type" {
  type    = string
  description = "Choose between LoadBalancer, NodePort, or ClusterIP for the service type."
  default = "ClusterIP"
}
