variable "REGISTRY" {}
variable "VERSION" {}
variable "IMAGE_REPOSITORY" {
  default = "infraweave-io/infraweave"
}
variable "PLATFORMS" {
  default = "linux/arm64"
}

target "internal-api-common" {
  context = "."
  dockerfile = "internal-api/Dockerfile.lambda.debian"
  args = {
    IMAGE_REPOSITORY = IMAGE_REPOSITORY
  }
}

target "internal-api-multiarch" {
  inherits = ["internal-api-common"]
  platforms = split(",", PLATFORMS)
  tags = ["${REGISTRY}/internal-api:${VERSION}"]
  attest = [
    "type=provenance,mode=max",
    "type=sbom"
  ]
}

target "internal-api-per-arch" {
  inherits = ["internal-api-common"]
  name     = "internal-api-${replace(item.platform, "linux/", "")}"
  matrix = {
    item = [for p in split(",", PLATFORMS) : {
      platform = p
      suffix   = replace(p, "linux/", "")
    }]
  }
  platforms  = [item.platform]
  tags       = ["${REGISTRY}/internal-api:${VERSION}-${item.suffix}"]
  attest = []
}

group "default" {
  targets = ["internal-api-multiarch", "internal-api-per-arch"]
}