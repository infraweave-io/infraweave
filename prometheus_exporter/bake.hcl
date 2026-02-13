variable "REGISTRY" {}
variable "VERSION" {}

target "prometheus_exporter" {
  context = "."
  dockerfile = "prometheus_exporter/Dockerfile.debian"
  tags = ["${REGISTRY}/prometheus_exporter:${VERSION}"]
  platforms = ["linux/arm64"]
}

group "default" {
  targets = ["prometheus_exporter"]
}
