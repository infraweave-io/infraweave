variable "REGISTRY" {}
variable "VERSION" {}
variable "IMAGE_REPOSITORY" {
  default = "infraweave-io/infraweave"
}

target "operator" {
  context = "."
  dockerfile = "operator/Dockerfile.debian"
  args = {
    IMAGE_REPOSITORY = IMAGE_REPOSITORY
  }
  tags = ["${REGISTRY}/operator:${VERSION}"]
  platforms = ["linux/arm64"]
}

group "default" {
  targets = ["operator"]
}
