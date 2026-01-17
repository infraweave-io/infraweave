variable "REGISTRY" {}
variable "VERSION" {}

target "terraform-stage-musl" {
  context = "."
  dockerfile = "terraform_runner/Dockerfile.terraform.alpine"
  target = "terraform"
  platforms = ["linux/arm64"]
}

target "tofu-stage-musl" {
  context = "."
  dockerfile = "terraform_runner/Dockerfile.tofu.alpine"
  target = "tofu"
  platforms = ["linux/arm64"]
}

target "opa-stage-musl" {
  context = "."
  dockerfile = "terraform_runner/Dockerfile.opa.alpine"
  target = "opa"
  platforms = ["linux/arm64"]
}

# Runner using terraform
target "runner-terraform-musl" {
  context = "."
  dockerfile = "terraform_runner/Dockerfile.runner.alpine"
  contexts = {
    terraform = "target:terraform-stage-musl"
    opa = "target:opa-stage-musl"
  }
  args = {
    REGISTRY_API_HOSTNAME = "registry.terraform.io"
  }
  tags = ["${REGISTRY}/runner:${VERSION}-terraform"]
  platforms = ["linux/arm64"]
}

# Runner using tofu (map tofu stage to terraform context)
target "runner-tofu-musl" {
  context = "."
  dockerfile = "terraform_runner/Dockerfile.runner.alpine"
  contexts = {
    terraform = "target:tofu-stage-musl"  # Map tofu stage to terraform context
    opa = "target:opa-stage-musl"
  }
  args = {
    REGISTRY_API_HOSTNAME = "registry.opentofu.org"
  }
  tags = ["${REGISTRY}/runner:${VERSION}-tofu"]
  platforms = ["linux/arm64"]
}

# Build both runners
group "default" {
  targets = ["runner-terraform-musl", "runner-tofu-musl"]
}

group "musl" {
  targets = ["runner-terraform-musl", "runner-tofu-musl"]
}

