variable "REGISTRY" {}
variable "VERSION" {}

target "webserver-openapi-gnu" {
  context = "."
  dockerfile = "webserver-openapi/Dockerfile.debian"
  tags = ["${REGISTRY}/webserver-openapi:${VERSION}"]
  platforms = ["linux/arm64"]
}

group "default" {
  targets = ["webserver-openapi-gnu"]
}

group "gnu" {
  targets = ["webserver-openapi-gnu"]
}

