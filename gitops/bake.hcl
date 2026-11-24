variable "REGISTRY" {}
variable "VERSION" {}
variable "IMAGE_REPOSITORY" {
  default = "infraweave-io/infraweave"
}

target "gitops-generic" {
  context = "."
  dockerfile = "gitops/Dockerfile.generic.debian"
  args = {
    IMAGE_REPOSITORY = IMAGE_REPOSITORY
  }
  tags = ["${REGISTRY}/gitops-generic:${VERSION}"]
  platforms = ["linux/arm64"]
}

target "gitops-aws" {
  context = "."
  dockerfile = "gitops/Dockerfile.lambda.debian"
  args = {
    IMAGE_REPOSITORY = IMAGE_REPOSITORY
  }
  tags = ["${REGISTRY}/gitops-aws:${VERSION}"]
  platforms = ["linux/arm64"]
}

group "default" {
  targets = ["gitops-aws"]
}
