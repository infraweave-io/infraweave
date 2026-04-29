variable "REGISTRY" {}
variable "VERSION" {}

target "internal-api-aws" {
  context = "."
  dockerfile = "internal-api/Dockerfile.lambda.debian"
  tags = ["${REGISTRY}/internal-api-aws:${VERSION}"]
  platforms = ["linux/arm64"]
}

target "internal-api-azure" {
  context = "."
  dockerfile = "internal-api/Dockerfile.azure.debian"
  tags = ["${REGISTRY}/internal-api-azure:${VERSION}"]
  platforms = ["linux/amd64"]
}

group "default" {
  targets = ["internal-api-aws"]
}
