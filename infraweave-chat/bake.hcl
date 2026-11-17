variable "REGISTRY" {}
variable "VERSION" {}
variable "PLATFORMS" {
  default = "linux/arm64"
}

target "infraweave-chat-common" {
  context = "."
  dockerfile = "infraweave-chat/Dockerfile.lambda.debian"
}

target "infraweave-chat-multiarch" {
  inherits = ["infraweave-chat-common"]
  platforms = split(",", PLATFORMS)
  tags = ["${REGISTRY}/infraweave-chat:${VERSION}"]
  attest = [
    "type=provenance,mode=max",
    "type=sbom"
  ]
}

target "infraweave-chat-per-arch" {
  inherits = ["infraweave-chat-common"]
  name     = "infraweave-chat-${replace(item.platform, "linux/", "")}"
  matrix = {
    item = [for p in split(",", PLATFORMS) : {
      platform = p
      suffix   = replace(p, "linux/", "")
    }]
  }
  platforms  = [item.platform]
  tags       = ["${REGISTRY}/infraweave-chat:${VERSION}-${item.suffix}"]
  attest = []
}

group "default" {
  targets = ["infraweave-chat-multiarch", "infraweave-chat-per-arch"]
}