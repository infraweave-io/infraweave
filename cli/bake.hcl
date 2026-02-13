variable "REGISTRY" {}
variable "VERSION" {}

target "cli" {
  context = "."
  dockerfile = "cli/Dockerfile.alpine"
  tags = ["${REGISTRY}/cli:${VERSION}"]
  platforms = ["linux/arm64"]
}

group "default" {
  targets = ["cli"]
}

