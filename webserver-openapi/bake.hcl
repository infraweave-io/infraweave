variable "REGISTRY" {}
variable "VERSION" {}

target "webserver-openapi" {
  context = "."
  dockerfile = "webserver-openapi/Dockerfile.debian"
  tags = ["${REGISTRY}/webserver-openapi:${VERSION}"]
  platforms = ["linux/arm64"]
}

group "default" {
  targets = ["webserver-openapi"]
}
