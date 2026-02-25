# Env Common

This package implements the cloud agnostic business logic and is one of the core modules.

## Provider

A **provider** is a Terraform provider package that can be published and versioned in the catalog. It is defined by a `provider.yaml` manifest. Providers are stored with metadata and a zip artifact.

```mermaid
flowchart LR

    subgraph provider_repo["Provider repo"]
        provider_manifest["provider.yaml"]
        provider_src["./src (tf)"]
    end

    subgraph iw_provider["Infraweave provider"]
        iwp_metadata["metadata"]
        subgraph iwp_bundle["bundle"]
            iwp_manifest["provider.yaml"]
            iwp_code["./src (tf)"]
        end
    end

    provider_repo --> iwp_bundle
```

## Module

A **module** is a reusable Terraform module published to the catalog. It is defined by a `module.yaml` manifest. Modules are stored with metadata and a zip. A module is built and run with its own root project that configures the provider and invokes the module. Modules can be downloaded by version and used as the building block in stacks.

```mermaid
flowchart LR
    subgraph iw_provider["Infraweave provider"]
        iwp_metadata["metadata"]
        subgraph iwp_bundle["bundle"]
            iwp_code["./src (tf)"]
        end
    end

    subgraph module_repo["Module repo"]
        module_manifest["module.yaml"]
        module_src["./src (tf)"]
    end

    subgraph iw_module["Infraweave module"]
        iwm_metadata["metadata"]
        subgraph iwm_root["root project"]
            iwm_provider["provider"]
            iwmr_variables["variables"]
            iwmr_outputs["outputs"]
            iwmr_module["module"]
        end
        subgraph iwm_bundle["bundle"]
            iwm_code["./src (tf)"]
        end
        iwmr_module -->|source| iwm_code
        iwm_provider -->|injected| iwm_code
        iwmr_variables -->|hoisted| iwm_code
        iwmr_outputs -->|hoisted| iwm_code
    end
    module_src ---> iwm_code
    module_manifest -->|specifies| iw_provider
    iwp_code --> iwm_provider
```

## Stack

A **stack** is a composition of one or more modules, defined by a `stack.yaml` manifest and claim manifests. Stacks are stored like modules with metadata and a zip. Publishing a stack merges providers from the claimed modules, generates a single Terraform root module from the claims, and publishes the result as a versioned catalog entry that can be deployed like a module. The stack root is created with the providers configured in the root and each claimed module invoked from that root.

```mermaid
flowchart LR

    subgraph stack_repo["Stack repo"]
        stack_manifest["stack.yaml"]
        stack_module_claims["Infraweave module claims..."]
    end

    subgraph iw_provider["Infraweave provider"]
        iwp_metadata["metadata"]
        subgraph iwp_bundle["bundle"]
            iwp_manifest["provider.yaml"]
            iwp_code["./src (tf)"]
        end
    end

    stack_repo ~~~ iw_module

    subgraph iw_module["Infraweave module"]
        iwm_metadata["metadata"]
        subgraph iwm_root["root project(discarded)"]
            iwmr_provider["provider"]
            iwmr_module["module"]
            iwmr_variables["variables"]
            iwmr_outputs["outputs"]
        end
        subgraph iwm_bundle["bundle"]
            iwm_manifest["module.yaml"]
            iwm_code["./src (tf)"]
        end
        iwmr_provider -->|injected| iwm_code
        iwmr_module -->|source| iwm_code
        iwmr_variables -->|hoisted| iwm_code
        iwmr_outputs -->|hoisted| iwm_code
    end

    subgraph iw_stack["Infraweave stack"]
        iws_metadata["metadata"]
        subgraph iws_root["root project"]
            iwsr_providers["providers"]
            iwsr_variables["variables"]
            iwsr_outputs["outputs"]
            iwsr_module["module*"]
        end
        subgraph iws_bundle["bundle"]
            iwsb_modules[modules*]
        end
        iwsr_module -->|src| iwsb_modules
        iwsr_providers -->|injected|iwsb_modules
        iwsr_variables ---|hoisted| iwsb_modules
        iwsr_outputs --- |hoisted|iwsb_modules
    end
    stack_module_claims ---->|specifies|iw_module
    iwm_bundle --> iwsb_modules
    iwm_metadata ---> iw_provider
    iwp_bundle --> iwsr_providers

    style iwm_root fill:#ff9999,stroke:#ff0000,stroke-width:2px,color:#fff
```
