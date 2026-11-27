variable "REGISTRY" {}
variable "VERSION" {}
variable "IMAGE_REPOSITORY" {
  default = "infraweave-io/infraweave"
}

target "cli" {
  context = "."
  dockerfile = "cli/Dockerfile.alpine"
  args = {
    IMAGE_REPOSITORY = IMAGE_REPOSITORY
  }
  tags = ["${REGISTRY}/cli:${VERSION}"]
  platforms = ["linux/arm64"]
}

group "default" {
  targets = ["cli"]
}

