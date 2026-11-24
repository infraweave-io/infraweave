variable "REGISTRY" {}
variable "VERSION" {}
variable "IMAGE_REPOSITORY" {
  default = "infraweave-io/infraweave"
}

target "reconciler-generic" {
  context = "."
  dockerfile = "reconciler/Dockerfile.generic.debian"
  args = {
    IMAGE_REPOSITORY = IMAGE_REPOSITORY
  }
  tags = ["${REGISTRY}/reconciler-generic:${VERSION}"]
  platforms = ["linux/arm64"]
}

target "reconciler-aws" {
  context = "."
  dockerfile = "reconciler/Dockerfile.lambda.debian"
  args = {
    IMAGE_REPOSITORY = IMAGE_REPOSITORY
  }
  tags = ["${REGISTRY}/reconciler-aws:${VERSION}"]
  platforms = ["linux/arm64"]
}

group "default" {
  targets = ["reconciler-aws"]
}
