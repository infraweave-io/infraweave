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

InfraWeave is an cloud-native control plane designed to minimize the gap between infrastructure as code (IaC) and the developer teams. With InfraWeave, you can simplify development of infrastructure templates, managing, updating, deploying it swiftly, easy and cost-effectively.

**Key features of InfraWeave include:**


- **🚀 Multi-Deploy Support**: Define your infrastructure using GitOps, CLI commands, Python scripts, or Kubernetes manifests, catering to diverse workflows.

- **⚙️ Terraform Engine**: Harness the reliability and flexibility of Terraform, a battle-tested tool for infrastructure provisioning.

- **🔗 Seamless Integrations**: Fully integrates with the Backstage Developer Portal and offers an API for custom integrations.

- **👩‍💻 Platform-Friendly**: Enables platform teams to publish, test, and upgrade existing Terraform modules effortlessly.

- **💡 Developer-First Deployment**: Simplifies infrastructure deployment for developers using prebuilt, reusable modules.

- **📄 Code-Coupled Documentation**: Ensures documentation stays accurate and aligned by directly integrating it with Terraform code and module/stack manifests.

- **🤝 Collaborative Stacks**: Facilitate collaboration by building tailored stacks for teams, ensuring safe and seamless upgrades.

- **🛠️ Minimal Maintenance**: Leverages a minimal set of managed services to significantly reduce operational overhead.

- **📈 Scalable by Design**: Built to scale seamlessly with cloud infrastructure, supporting everything from small projects to enterprise-level deployments.

- **💸 Cost-Efficient**: Optimized for usage, typically costing only a few dollars per month, making it accessible for teams of all sizes.

- **🌟 Open Source**: Join a thriving community to shape the future of infrastructure together—let’s build it collaboratively! 🎉


View the [features](https://preview.infraweave.io/core-concepts/key-features/) and [documentation](https://preview.infraweave.io/core-concepts/overview/).

<h2>𝌞&nbsp;&nbsp;Contents</h2>

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

<h2>📖&nbsp;&nbsp;Documentation</h2>

To read the up-to-date documentation, please check out our [documentation](https://preview.infraweave.io/core-concepts/modules/)

<h2>📖&nbsp;&nbsp;Current Status</h2>

The project is currently in **preview** 👀

<h2>🟩&nbsp;&nbsp;Getting Started</h2>

### Setting up the platform

To bootstrap your cloud, set up the central and workload modules for your desired cloud provider, [find them here](http://localhost:4321/getting-started/links/#repositories).

You need to set up:

* **central** - storage and databases required by the control plane
* **workload** - runtime environments which should be deployed per project (e.g. AWS Account/Azure Subscription)

### Publish a module

It all starts with you having a Terraform module available that you want to deploy.

0. Have a terraform module ready (including the lockfile `.terraform.lock.hcl`).

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
  name: s3bucket # The name of the module you define
spec:
  moduleName: S3Bucket # metadata.name cannot have any uppercase, which is why we need this
  version: 0.0.11-dev # The released version to use
  description: "This module deploys an S3 bucket in AWS" # Supports markdown
  reference: https://github.com/your-org/s3bucket # The URL to the module's source code
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
  namespace: default
spec:
  moduleVersion: 0.0.11-dev # The released version to use, must match the version in the module.yaml
  region: us-west-2
  variables:
    bucketName: my-unique-bucket-name-32142j
    tags:
      Name234: my-s3bucket
      Environment43: dev
```

* GitOps

Given that it is [configured](https://preview.infraweave.io/gitops), simply push the claim to your repository, that’s it! 🎉

* CLI

In case you want to set something up quick and dirty from your local computer, this is easy:

Using the same manifest file as above

```sh
infraweave apply <some-namespace-here> s3_manifest.yaml
```

* Kubernetes

Given you have installed the [operator](https://preview.infraweave.io/kubernetes/) you might want to create an S3 Bucket next to your application in a Kubernetes cluster, this is as simple as this:

```sh
kubectl apply -f s3_manifest.yaml
```

* Python

Use python to set up you infrastructure readily available from the platform.

```python
from infraweave import S3Bucket, Deployment

bucket_module = S3Bucket(
    version='0.0.11-dev',
    track="dev"
)

bucket1 = Deployment(
    name="bucket1",
    environment="playground",
    module=bucket_module,
    region="us-west-2"
)

bucket1.set_variables(
    bucket_name="my-bucket12347ydfs3"
)

try:
  bucket1.apply()
  # Run some tests here
except Exception as e:
    print(f"An error occurred: {e}")
    # Handle the error as needed
finally:
  bucket1.destroy()
```

> *This can also be used to create integration tests with multiple modules or stacks*

<h2>🧑‍💻&nbsp;&nbsp;Community</h2>

Join our growing community around the world, for help, ideas, and discussions regarding InfraWeave.

- Chat live with us on [Discord](https://discord.gg/NWNE8ZXaRq)
<!-- - Connect with us on [LinkedIn](https://www.linkedin.com/company/infraweave/)
- Visit us on [YouTube](https://www.youtube.com/@infraweave)
- Join our [Dev community](https://dev.to/infraweave)
- Questions tagged #infraweave on [Stack Overflow](https://stackoverflow.com/questions/tagged/infraweave)
- Follow us on [X](https://x.com/infraweave) -->

<h2>🛟&nbsp;&nbsp;Contributing</h2>

We would ❤️ for you to get involved with InfraWeave development! If you wish to help, you can learn more about how you can contribute to this project in the [contribution guide](CONTRIBUTING.md).

<h2>🔒&nbsp;&nbsp;Security</h2>

For security issues, kindly email us at [opensource@infraweave.com](mailto:opensource@infraweave.com) instead of posting a public issue on GitHub.

<h2>🪪&nbsp;&nbsp;License</h2>

Source code for InfraWeave is released under the [Apache Licence 2.0](/LICENSE).
