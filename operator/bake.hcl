variable "REGISTRY" {}
variable "VERSION" {}

target "operator" {
  context = "."
  dockerfile = "operator/Dockerfile.debian"
  tags = ["${REGISTRY}/operator:${VERSION}"]
  platforms = ["linux/arm64"]
}

group "default" {
  targets = ["operator"]
}
