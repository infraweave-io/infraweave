variable "REGISTRY" {}
variable "VERSION" {}

target "reconciler-generic-gnu" {
  context = "."
  dockerfile = "reconciler/Dockerfile.generic.debian"
  tags = ["${REGISTRY}/reconciler-generic:${VERSION}"]
  platforms = ["linux/arm64"]
}

target "reconciler-aws-gnu" {
  context = "."
  dockerfile = "reconciler/Dockerfile.lambda.debian"
  tags = ["${REGISTRY}/reconciler-aws:${VERSION}"]
  platforms = ["linux/arm64"]
}

group "default" {
  targets = ["reconciler-aws-gnu"]
}

group "gnu" {
  targets = ["reconciler-aws-gnu"]
}
