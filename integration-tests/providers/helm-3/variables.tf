variable "kubernetes_endpoint" {
  type = string
  description = "Endpoint for communicating with kubernetes"
}

variable "kubernetes_ca_certificate" {
  type = string
  description = "Certificat used when communicating with kubernetes"
}

variable "kubernetes_token" {
  type = string
  description = "Authentication token used with kubernetes"
}