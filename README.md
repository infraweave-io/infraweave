# InfraWeave
<div style="display: flex; align-items: center; justify-content: center; gap: 10px;" align="center">
    <a href="https://preview.infraweave.io" target="_blank">
        <img width="20%" src="https://preview.infraweave.io/img/infrabridge-logo.png" alt="InfraWeave">
    </a>
</div>
<br>
<p align="center">
    <a href=""><img src="https://img.shields.io/github/v/release/infraweave-io/infraweave?color=ff00a0&include_prereleases&label=version&sort=semver&style=flat-square"></a>
    &nbsp;
    <a href=""><img src="https://img.shields.io/badge/built_with-Rust-dca282.svg?style=flat-square"></a>
    &nbsp;
	<a href="https://github.com/infraweave-io/infraweave/actions"><img src="https://img.shields.io/github/actions/workflow/status/infraweave-io/infraweave/docker-cli.yml?style=flat-square&branch=main"></a>
    &nbsp;
    <a href="https://github.com/infraweave-io/infraweave/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-Apache_2.0-00bfff.svg?style=flat-square"></a>
</p>

<p align="center">
    <a href="https://hub.docker.com/u/infraweave"><img src="https://img.shields.io/docker/pulls/infraweave/runner?label=docker%20pulls&style=flat-square"></a>
    &nbsp;
    <!-- <a href="https://www.npmjs.com/package/infraweave"><img src="https://img.shields.io/npm/dt/infraweave.js?color=f7df1e&label=javascript&style=flat-square"></a>
    &nbsp; -->
	<a href="https://pypi.org/project/infraweave/"><img src="https://img.shields.io/pepy/dt/infraweave?color=426c99&label=python&style=flat-square"></a>
</p>

<p align="center">
	<a href="https://discord.gg/NWNE8ZXaRq"><img src="https://img.shields.io/discord/1332995139567878246?label=discord&style=flat-square&color=5a66f6" alt="Discord"></a>
	&nbsp;
    <!-- <a href="https://x.com/infraweave"><img src="https://img.shields.io/badge/x-follow_us-222222.svg?style=flat-square" alt="X"></a>
    &nbsp; -->
    <!-- <a href="https://www.linkedin.com/company/infraweave/"><img src="https://img.shields.io/badge/linkedin-connect_with_us-0a66c2.svg?style=flat-square" alt="LinkedIn"></a>
	&nbsp; -->
    <!-- <a href="https://www.youtube.com/@infraweave"><img src="https://img.shields.io/badge/youtube-subscribe-fc1c1c.svg?style=flat-square" alt="YouTube"></a> -->
</p>

<br>

<h2><img height="20" src="https://preview.infraweave.io/img/infrabridge-logo.png">&nbsp;&nbsp;What is InfraWeave?</h2>

InfraWeave is a cloud-native control plane that bridges the gap between infrastructure as code (IaC) and development teams. It simplifies how you build, manage, and deploy infrastructure templates.

**Key features:**

- **Multiple Deployment Methods**: Deploy infrastructure via GitOps, CLI, Python SDK, or Kubernetes manifests.

- **Terraform-Powered**: Built on Terraform for reliable, production-ready infrastructure provisioning.

- **Integration Ready**: Works with Backstage Developer Portal and provides APIs for custom integrations.

- **Platform Team Enablement**: Publish, version, and upgrade Terraform modules with minimal friction.

- **Developer-Focused**: Deploy infrastructure using prebuilt modules without deep Terraform expertise.

- **Documentation as Code**: Module documentation lives alongside your Terraform code, staying in sync automatically.

- **Stack Composition**: Build and share infrastructure stacks across teams with safe upgrade paths.

- **Low Maintenance**: Runs on a minimal set of managed services to reduce operational overhead.

- **Scales With You**: Handles everything from small projects to enterprise infrastructure.

- **Cost-Effective**: Typically runs for a few dollars per month.

View the [features](https://preview.infraweave.io/core-concepts/key-features/) and [documentation](https://preview.infraweave.io/core-concepts/overview/).

<h2>Contents</h2>

- [Documentation](#documentation)
- [Current Status](#current-status)
- [Getting started](#getting-started)
	- [Setting up the platform](#setting-up-the-platform)
	- [Publish a module](#publish-a-module)
  - [Deploy an available module](#deploy-an-available-module)
- [Community](#community)
- [Contributing](#contributing)
- [Security](#security)
- [License](#license)

<h2>Documentation</h2>

For detailed documentation, visit [preview.infraweave.io](https://preview.infraweave.io/core-concepts/modules/).

<h2>Current Status</h2>

InfraWeave is currently in preview.

<h2>Getting Started</h2>

### Setting up the platform

Bootstrap your cloud environment by deploying the central and workload modules for your cloud provider. [Repository links](https://preview.infraweave.io/getting-started/links/#repositories).

Required components:

* **central** - Storage and databases for the control plane
* **workload** - Runtime environments deployed per project (e.g., AWS Account/Azure Subscription)

### Publish a module

*Prerequisites: the following example assumes a provider has already been published*

Start with a Terraform module you want to make available for deployment.

0. Prepare your Terraform module (no lock-file as this is generated when published).

```tf
terraform {
  required_providers {
    aws = {
      source = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

resource "aws_s3_bucket" "example" {
  bucket = var.bucket_name
  tags   = var.tags
}

variable "bucket_name" {
  type    = string
}

variable "tags" {
  type = map(string)
  default = {
    Owner = "John Doe"
    Department = "Platform"
  }
}
```

1. Define a `module.yaml`

```yaml
apiVersion: infraweave.io/v1
kind: Module
metadata:
  name: s3bucket # The name of the module you define (lowercase)
spec:
  moduleName: S3Bucket # metadata.name cannot have any uppercase, which is why we need this
  version: 0.0.11-dev # The released version to use
  reference: https://github.com/your-org/s3bucket # The URL to the module's source code
  providers:
    - name: aws-6 # This is published separately and defined similar to a module
  description: |
    # S3Bucket module
    This module deploys an S3 bucket in AWS
```

2. Publish it! *(to dev-track)*

```sh
infraweave module publish dev .
```

#### Deploy an available module

Let’s look at four different ways to deploy this module:
* GitOps
* CLI
* Kubernetes
* Python

For the first three options, you will use a manifest like this:

```yaml
apiVersion: infraweave.io/v1
kind: S3Bucket
metadata:
  name: my-s3-bucket
spec:
  moduleVersion: 0.0.11-dev # The released version to use, must match the version in the module.yaml
  region: us-west-2
  variables:
    bucketName: my-unique-bucket-name-32142j
    tags:
      Name234: my-s3bucket
      Environment43: dev
```

**GitOps**

With GitOps [configured](https://preview.infraweave.io/gitops), push the manifest to your repository:

```sh
git add s3_manifest.yaml
git commit -m "Add S3 bucket"
git push
```

**CLI**

For quick local deployments:

```sh
infraweave apply <namespace> s3_manifest.yaml
```

**Kubernetes**

With the [operator installed](https://preview.infraweave.io/kubernetes/), deploy alongside your application:

```sh
kubectl apply -f s3_manifest.yaml
```

**Python**

Deploy infrastructure programmatically:

```python
from infraweave import S3Bucket, Deployment

bucket_module = S3Bucket(
    version='0.0.11-dev',
    track="dev"
)

bucket1 = Deployment(
    name="bucket1",
    module=bucket_module,
    region="us-west-2"
)

with bucket1:
    bucket1.set_variables(
        bucket_name="my-bucket12347ydfs3"
    )
    bucket1.apply()
    # Run tests or perform operations

# Automatic cleanup on context exit
```

The Python SDK is useful for integration tests involving multiple modules or stacks.

<h2>Community</h2>

Join the InfraWeave community for help, ideas, and discussions:

- [Discord](https://discord.gg/NWNE8ZXaRq) - Chat with the team and other users
<!-- - [LinkedIn](https://www.linkedin.com/company/infraweave/)
- [YouTube](https://www.youtube.com/@infraweave)
- [Dev community](https://dev.to/infraweave)
- [Stack Overflow](https://stackoverflow.com/questions/tagged/infraweave) - Questions tagged #infraweave
- [X](https://x.com/infraweave) -->

<h2>Contributing</h2>

Contributions are welcome! See the [contribution guide](CONTRIBUTING.md) to get started.

<h2>Security</h2>

Report security vulnerabilities to [opensource@infraweave.com](mailto:opensource@infraweave.com) rather than creating public issues.

<h2>License</h2>

InfraWeave is released under the [Apache License 2.0](/LICENSE).
