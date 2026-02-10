variable "REGISTRY" {}
variable "VERSION" {}

target "gitops-generic" {
  context = "."
  dockerfile = "gitops/Dockerfile.generic.debian"
  tags = ["${REGISTRY}/gitops-generic:${VERSION}"]
  platforms = ["linux/arm64"]
}

target "gitops-aws" {
  context = "."
  dockerfile = "gitops/Dockerfile.lambda.debian"
  tags = ["${REGISTRY}/gitops-aws:${VERSION}"]
  platforms = ["linux/arm64"]
}

group "default" {
  targets = ["gitops-aws"]
}
