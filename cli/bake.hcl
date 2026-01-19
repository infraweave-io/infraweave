variable "REGISTRY" {}
variable "VERSION" {}

target "cli-musl" {
  context = "."
  dockerfile = "cli/Dockerfile.alpine"
  tags = ["${REGISTRY}/cli:${VERSION}"]
  platforms = ["linux/arm64"]
}

group "default" {
  targets = ["cli-musl"]
}

group "musl" {
  targets = ["cli-musl"]
}

