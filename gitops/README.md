# GitOps

This package is a minimal serverless (push-based) gitops module, used to integrate with InfraWeave using webhooks.
Large parts are shared but contains differences in the data format for different providers.

## Brief

GitOps is a method where you can rely on git as being the source of truth, this means that it has to be aligned with the reality.

There are three possible states:
* git
* InfraWeave
* Reality

### git <-> InfraWeave

This implies there are two parts necessary to not get drift:
* Something is modified in git => Will trigger a change in InfraWeave
* Someone has manually made changes to InfraWeave deployment => Needs to be periodically checked (TODO: work in progress 🔨)

### InfraWeave <-> Reality

* Something is modified in InfraWeave => Triggers a job to reconcile
* Someone has manually made changes to the infrastructure => Can be fixed by setting reconcile in your claim

> Note: this is a preview version

## Supported Cloud Providers:

* ✅ AWS
* ➖ Azure

## Supported Git Providers:

* ✅ GitHub
* ➖ Gitlab

Please create an [issue](https://github.com/infraweave-io/infraweave/issues) if you are missing something
