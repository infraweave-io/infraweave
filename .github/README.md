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

#### How it works in integration tests

Integration tests start several container images (LocalStack, MinIO, Azurite, and others). To avoid Docker Hub throttling, CI can pull those images from this repo’s GHCR mirror instead.

- **Environment variable:** `DOCKER_IMAGE_MIRROR` must be a **registry prefix only** (no JSON), for example `ghcr.io/owner/repo`. In `.github/workflows/test.yml`, the integration-test job sets it to `ghcr.io/${{ github.repository }}` so forks automatically target their own GHCR namespace. For local runs, export the same shape if you want mirrored pulls.
- **Code:** `integration-tests/src/scaffold.rs` defines a private helper `get_image_name(original_image, tag)`.
  - If `DOCKER_IMAGE_MIRROR` is unset or empty, it returns `(original_image, tag)` unchanged (upstream image references).
  - If it is set, it matches `original_image` against a fixed list of upstream names (for example `localstack/localstack`, `minio/minio`) and maps each to a **short mirror basename** (for example `localstack`, `minio`). That basename must match the image part of the `to` field in `.github/vars/default.image_mirror.json` (e.g. `to: "localstack:3.0"` → basename `localstack`). It then returns `(format!("{prefix}/{basename}"), tag)`.
  - Any upstream image **not** handled in that `match` is left as the original reference even when the prefix is set, so new services need an explicit mapping in code.

#### Populate the mirror

**Mirror Docker Images to GHCR** (`image_mirror.yml`) is a standalone workflow you run manually (`workflow_dispatch`). It pulls each `from` image in the matrix, retags it as `ghcr.io/<repository>/<to>`, and pushes to GHCR, so images are cached under your repo and pulls in CI hit GHCR instead of Docker Hub (or other upstream registries).

- **Matrix:** `.github/scripts/image_mirror_setup-matrix.sh` always loads `.github/vars/default.image_mirror.json`. If the GitHub Actions repository variable **`DOCKER_IMAGE_MIRROR`** is set, it must be a **JSON array** of `{ "from", "to" }` objects; those entries are **merged** with the default, and any row with the same `from` as a default entry **replaces** that default (see the script’s `jq` merge).
- **Docker Hub (optional):** Set secrets **`DOCKERHUB_USERNAME`** and **`DOCKERHUB_TOKEN`** if you want authenticated pulls and higher rate limits; the workflow logs in to Docker Hub only when `DOCKERHUB_USERNAME` is non-empty.

Run this workflow after changing the matrix (or periodically) so GHCR contains the tags your tests resolve to.

#### Adding a new image

##### Fork or private use only

- Extend the mirror matrix via the repository variable **`DOCKER_IMAGE_MIRROR`** (JSON array merged as above), or edit `.github/vars/default.image_mirror.json` on your fork if you prefer not to use the variable.
- Add a **new arm** to the `match` in `get_image_name` in `integration-tests/src/scaffold.rs` so the upstream `original_image` maps to the same short name as the `to` field’s image component. Without that change, tests keep pulling the upstream image even when the mirror job pushes to GHCR.
- Ensure GHCR packages are visible to the workflows or actors that pull them (package permissions), then run **Mirror Docker Images to GHCR** and run integration tests with `DOCKER_IMAGE_MIRROR` pointing at your `ghcr.io/owner/repo` prefix.

##### Contributing back to infraweave

- Add a `{ "from", "to" }` entry to `.github/vars/default.image_mirror.json` using the same naming convention as existing rows (`to` is `basename:tag` under this repository’s GHCR path).
- Extend `get_image_name` in `integration-tests/src/scaffold.rs` with the new upstream image string and matching short basename.
- Open a PR so default CI and the mirror workflow stay in sync.

### Documentation: `docs.yml`

**Build & Deploy Documentation** runs on push and pull_request to `main`. It builds the MkDocs site (Rust + Python, CLI reference, MkDocs Material), runs on the default branch only for deployment.

- **On push to `main`:** Builds the site, uploads the artifact, and deploys to **GitHub Pages**.
- **On pull requests (to `main`):** Builds the site and uploads a **docs-preview** artifact (7-day retention); if the PR is from the same repo, it comments on the PR with instructions to download and view the preview.
- **Rust cache:** Uses `save-if` on the repository default branch (see [Caching](#caching)).

---

## Caching

Caching (Rust cache, Docker layer cache, cibuildwheel cache, etc.) is configured so that **cache, if saved, is only saved (written) on the repository’s default branch**. All branches can **restore** cache; only the default branch **saves** it. This avoids filling the cache with many branch-specific entries and keeps cache keys stable.

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
| **image_mirror_setup-matrix.sh** | Builds image-mirror matrix from default + JSON in `DOCKER_IMAGE_MIRROR`. |
| **lint_clippy-2-md.sh** | Runs `cargo clippy` (all crates or one), converts output to markdown for GITHUB_STEP_SUMMARY. |
| **jq-to-markdown.jq** | jq script used for formatting. |

Tests for these scripts live in `.github/scripts_test/` (e.g. `test_calculate-version.sh`, `test_*_setup-build-matrix.sh`).

---

## Variables and defaults

- **Repository / organization variables** (e.g. in GitHub Settings → Secrets and variables → Actions) can override defaults:
  - `TARGETS`, `BINARIES`, `DOCKER_IMAGES`, `PYTHON_WHEELS`, etc.
  - **`DOCKER_IMAGE_MIRROR`** – Optional **JSON array** for `image_mirror.yml` only: merged into `.github/vars/default.image_mirror.json` by `image_mirror_setup-matrix.sh` (details under **Image mirror** above). Integration tests use the **same name as an environment variable** with a different meaning (GHCR prefix); in this repo that env is set in `test.yml`, not from this var.
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
```

Replace `master` with your actual default branch everywhere (in `on.push.branches`, `on.pull_request.branches`, `release_branch`, and `if`). If your reusable workflows need repository secrets, add `secrets: inherit` to each `uses` job (same indentation as `with:`).

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
