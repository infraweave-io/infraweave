variable "REGISTRY" {}
variable "VERSION" {}

target "reconciler-generic" {
  context = "."
  dockerfile = "reconciler/Dockerfile.generic.debian"
  tags = ["${REGISTRY}/reconciler-generic:${VERSION}"]
  platforms = ["linux/arm64"]
}

target "reconciler-aws" {
  context = "."
  dockerfile = "reconciler/Dockerfile.lambda.debian"
  tags = ["${REGISTRY}/reconciler-aws:${VERSION}"]
  platforms = ["linux/arm64"]
}

group "default" {
  targets = ["reconciler-aws"]
}
