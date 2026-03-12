# GitHub Actions: Workflows, Scripts & Fork Setup

This document describes the CI/CD layout under `.github/`, how workflows and scripts fit together, and how to customize and run the same pipeline on a fork (e.g. on your own default branch).

---

## Workflows

### Entry-point workflow: `ci_main.yml`

**CI (main)** is the main entry point for this repository. It runs on:

- **Push** and **pull_request** to the `main` branch
- **workflow_dispatch** with a mode: `build`, `pre-release`, or `release`

It does two things:

1. **Run CI** – Calls the reusable `ci.yml` workflow with:
   - `release_branch: main`
   - `release` / `pre_release` according to the chosen mode
2. **Release** (only on `main` when mode is `release` or `pre-release`) – Calls `release.yml` to create the GitHub release and optionally publish to PyPI.

So for **this** repo, “the main branch” is `main`. In a fork, you would add a similar entry-point workflow that uses **your** default branch (see [Fork setup](#fork-setup)).

### Reusable workflows (called by `ci.yml` or directly)

| Workflow | Purpose |
|----------|---------|
| **ci.yml** | Orchestrates: version calculation → build binaries, tests, Docker, wheels → optional release. Accepts `release_branch`, `release`, `pre_release`. |
| **calculate-version.yml** | Computes semantic version from tags and commits; outputs `version`, `base_tag`, `commit_count`. Uses `release_branch`. |
| **binaries.yml** | Builds CLI binaries for targets from `vars.TARGETS` / `vars.BINARIES`. |
| **test.yml** | Runs tests (lint, clippy, build-tests, integration). |
| **docker.yml** | Builds and optionally pushes Docker images; uses version and `release_branch`; respects `allow_push`. |
| **wheels-maturin-action.yml** | Builds Python wheels. |
| **release.yml** | Creates GitHub Release, uploads binaries/wheels; optionally publishes wheels to PyPI. |

### Image mirror: `image_mirror.yml`

**Mirror Docker Images to GHCR** is a standalone workflow (triggered manually via `workflow_dispatch`). It pulls images from Docker Hub (and other registries) and pushes them to the repository’s GitHub Container Registry (GHCR). This is used for **caching images** and avoiding **rate limits / throttling** when CI pulls the same images repeatedly (e.g. in integration tests).

- **Variable:** The workflow uses the repository variable **`DOCKER_IMAGE_MIRROR`** (or the default from `.github/vars/default.image_mirror.json` via the script). The matrix is built by `.github/scripts/image_mirror_setup-matrix.sh` (env `IMAGE_MIRROR` = `vars.DOCKER_IMAGE_MIRROR`).
- **Integration tests:** The integration tests use the **same variable as an environment variable**: **`DOCKER_IMAGE_MIRROR`**. When set, `integration-tests/tests/utils.rs` (e.g. `get_image_name`) resolves container image names to the mirrored names (e.g. `ghcr.io/owner/repo/localstack:3.0` instead of `localstack/localstack:3.0`). So after you run the image-mirror workflow and set `DOCKER_IMAGE_MIRROR` (e.g. to `ghcr.io/<owner>/<repo>`), CI and integration tests pull from GHCR instead of Docker Hub, avoiding throttling and using your mirrored images.
- **Secrets (optional):** For higher Docker Hub rate limits, set `DOCKERHUB_USERNAME` and `DOCKERHUB_TOKEN`; the workflow logs in when these are present.

**Adding a new image:**

- **If you intend to contribute back:** Add the image to `.github/vars/default.image_mirror.json` (with `from` and `to` entries) and update the `get_image_name` function in `integration-tests/tests/utils.rs` so the new image is mapped to its mirrored name when `DOCKER_IMAGE_MIRROR` is set.
- **If not contributing back:** Add the image via the repository variable **`DOCKER_IMAGE_MIRROR`** (as JSON that extends or overrides the default matrix; see `.github/scripts/image_mirror_setup-matrix.sh`). You still need to update **`get_image_name`** in `integration-tests/tests/utils.rs` to handle the new lookup—otherwise integration tests will not resolve the mirrored image name for that image.

### Documentation: `docs.yml`

**Build & Deploy Documentation** runs on push and pull_request to `main`. It builds the MkDocs site (Rust + Python, CLI reference, MkDocs Material), runs on the default branch only for deployment.

- **On push to `main`:** Builds the site, uploads the artifact, and deploys to **GitHub Pages**.
- **On pull requests (to `main`):** Builds the site and uploads a **docs-preview** artifact (7-day retention); if the PR is from the same repo, it comments on the PR with instructions to download and view the preview.
- **Rust cache:** Uses `save-if` on the repository default branch (see [Caching](#caching)).

---

## Caching

Caching (Rust cache, Docker layer cache, cibuildwheel cache, etc.) is configured so that **cache is only saved (written) on the repository’s default branch**. All branches can **restore** cache; only the default branch **saves** it. This avoids filling the cache with many branch-specific entries and keeps cache keys stable.

- **Where this is used:** `save-if: ${{ github.ref == format('refs/heads/{0}', github.event.repository.default_branch) }}` (or equivalent) appears in:
  - **test.yml**, **docs.yml**, **binaries.yml**, **wheels-maturin-action.yml**, **tests.yaml** – Rust cache (`Swatinem/rust-cache`) only saves on the default branch.
  - **docker.yml** – `cache-image` and `cache-binary` are enabled only on the default branch; Docker layer cache is written only there.
  - **tests.yaml** – cibuildwheel save step runs only on the default branch.

**Implication for forks:** If your fork uses a **different** branch as its development or release branch (e.g. `master` or `trunk`), you should **set that branch as the repository’s default branch** in GitHub (Settings → General → Default branch). Then cache will be saved on your main branch and reused across runs. If you keep the upstream default (e.g. `main`) but do most work on another branch, that other branch will never save cache and will always do cold builds.

---

## Scripts

Scripts under `.github/scripts/` are used by the workflows. They rely on repository defaults (e.g. `vars/`) when corresponding env/vars are not set.

| Script | Purpose |
|--------|---------|
| **calculate-version.sh** | Derives semantic version from last tag and commits (conventional commits); writes `version`, `base_tag`, `commit_count` to `GITHUB_OUTPUT`. |
| **binaries_validate-targets.sh** | Ensures all targets referenced in `BINARIES` exist in `TARGETS`. |
| **binaries_setup-build-matrix.sh** | Builds the build-matrix JSON from `TARGETS` and `BINARIES`. |
| **docker_validate-binaries.sh** | Validates that Docker image configs have required linux-musl binaries. |
| **docker_setup-build-matrix.sh** | Builds Docker build matrix from `DOCKER_IMAGES` (or default). |
| **wheels_validate-targets.sh** | Validates wheel targets against `TARGETS`. |
| **wheels_setup-build-matrix.sh** | Builds wheel build matrix from `TARGETS` and `PYTHON_WHEELS`. |
| **image_mirror_setup-matrix.sh** | Builds image-mirror matrix from default + `IMAGE_MIRROR`. |
| **lint_clippy-2-md.sh** | Runs `cargo clippy` (all crates or one), converts output to markdown for GITHUB_STEP_SUMMARY. |
| **jq-to-markdown.jq** | jq script used for formatting. |

Tests for these scripts live in `.github/scripts_test/` (e.g. `test_calculate-version.sh`, `test_*_setup-build-matrix.sh`).

---

## Variables and defaults

- **Repository / organization variables** (e.g. in GitHub Settings → Secrets and variables → Actions) can override defaults:
  - `TARGETS`, `BINARIES`, `DOCKER_IMAGES`, `PYTHON_WHEELS`, `IMAGE_MIRROR`, etc.
  - **`DOCKER_IMAGE_MIRROR`** – Used by `image_mirror.yml` and by integration tests (`integration-tests/tests/utils.rs`) to pull mirrored images from GHCR instead of Docker Hub (caching, throttling). Set to your GHCR prefix (e.g. `ghcr.io/owner/repo`) when using the image mirror workflow.
  - `VERSION_STABLE`, `RELEASE_ASSETS`, `PYPI_PUBLISH`.
- **Default JSON configs** under `.github/vars/`:
  - `default.targets.json` – Rust/runner targets (e.g. linux-amd64, macos-arm64).
  - `default.binaries.json`, `default.docker.json`, `default.python_wheels.json`, `default.image_mirror.json` – used when the corresponding vars are not set.

Workflows pass these vars into the scripts (see e.g. `binaries.yml` with `vars.TARGETS`, `vars.BINARIES`).

---

## Fork setup

To run the same CI and (optionally) release flow on **your** fork with **your** default branch (e.g. `main` or `master`), do the following.

### 1. Add an entry-point workflow for your branch

`ci_main.yml` is tied to **this** repo’s `main`. In a fork, add a workflow that triggers on **your** default branch and calls the same reusable workflows with that branch name.

**Option A – Same branch name (`main`)**  
If your fork’s default branch is also `main`, you can keep using `ci_main.yml` as-is (it will run on your fork’s `main`).

**Option B – Different branch name (e.g. `master` or `trunk`)**  
Create a new file, e.g. `.github/workflows/ci_fork_main.yml`, modeled on `ci_main.yml` but with your branch and name:

```yaml
name: CI (fork main)

on:
  push:
    branches: [master]   # or your default branch
  pull_request:
    branches: [master]
  workflow_dispatch:
    inputs:
      mode:
        description: "Run mode"
        required: true
        type: choice
        options:
          - build
          - pre-release
          - release
        default: build

jobs:
  run-ci:
    name: Run CI (${{ inputs.mode || 'build' }})
    permissions:
      contents: read
      packages: write
    uses: ./.github/workflows/ci.yml
    with:
      release_branch: master   # must match the branch above
      release: ${{ inputs.mode == 'release' || inputs.mode == 'pre-release' }}
      pre_release: ${{ inputs.mode == 'pre-release' }}
    secrets: inherit

  release:
    name: Release (${{ needs.run-ci.outputs.version }})
    needs: run-ci
    if: ${{ github.ref == 'refs/heads/master' && (inputs.mode == 'release' || inputs.mode == 'pre-release') }}
    permissions:
      contents: write
      id-token: write
    uses: ./.github/workflows/release.yml
    with:
      base_tag: ${{ needs.run-ci.outputs.base_tag }}
      version: ${{ needs.run-ci.outputs.version }}
      release: ${{ inputs.mode == 'release' || inputs.mode == 'pre-release' }}
      pre_release: ${{ inputs.mode == 'pre-release' }}
    secrets: inherit
```

Replace `master` with your actual default branch everywhere (in `on.push.branches`, `on.pull_request.branches`, `release_branch`, and `if`).

### 2. Customize behavior (optional)

- **No releases from the fork**  
  Don’t run with mode `release` / `pre-release`, or remove/disable the `release` job in your fork’s workflow. You can still use `build` and `pre-release` for testing.

- **Release but no PyPI**  
  In the fork’s repo variables, either leave `PYPI_PUBLISH` unset or set it to something other than `'true'`. The release job will create the GitHub Release but skip PyPI (see `release.yml`).

- **Different targets/images/wheels**  
  Set repository variables `TARGETS`, `BINARIES`, `DOCKER_IMAGES`, `PYTHON_WHEELS`, etc., or change the JSON files under `.github/vars/` in your fork. Scripts will pick them up as documented above.

- **Version calculation**  
  `calculate-version.yml` and `calculate-version.sh` use `release_branch` to decide release vs dev versions. Passing your default branch as `release_branch` (as in the example) keeps behavior consistent.

### 3. Summary

| Goal | Action |
|------|--------|
| Run CI on fork’s default branch | Use `ci_main.yml` if branch is `main`, else add a `ci_fork_main.yml`-style workflow with your branch. |
| Use same builds/tests/release flow | Call `ci.yml` with `release_branch` set to your default branch. |
| Have cache saved on your main branch | Set your development/release branch as the repo’s **default branch** (Settings → General → Default branch). See [Caching](#caching). |
| Disable publishing | Avoid `release`/`pre-release` mode or set `PYPI_PUBLISH != true`. |
| Customize targets/images | Set vars or edit `.github/vars/*.json` in the fork. |

No changes to the reusable workflows (`ci.yml`, `release.yml`, etc.) or scripts are required for a typical fork; only the entry-point workflow (and optionally repo variables) need to reflect your branch and preferences.
