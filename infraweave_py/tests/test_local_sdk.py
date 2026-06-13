"""Tests for the compiled `infraweave` module against the local backend started
by the Rust harness (integration-tests/tests/python_sdk.rs), which seeds the
aws-5 provider and the s3bucketsimple module (track stable, version 1.0.0).
"""

import os

import pytest

# `import infraweave` hits the backend at import time to build a dynamic class
# per discovered module, so it must be reachable here. We only run inside the
# harness container (backend always up), so an unreachable backend should fail
# loudly at import rather than silently skip the suite.
import infraweave

# Matches what internal-api's local_setup seeds.
SEED_MODULE = "s3bucketsimple"
SEED_VERSION = "1.0.0"
SEED_TRACK = "stable"

# Always-present base classes; anything else exposed as a class is a generated
# per-module/stack wrapper.
_BASE_CLASSES = {"Module", "Stack", "Deployment", "PlanResult", "DeploymentResult"}


def _dynamic_module_classes():
    return {
        name: obj
        for name, obj in vars(infraweave).items()
        if isinstance(obj, type) and name not in _BASE_CLASSES and not name.startswith("_")
    }


def test_seeded_module_discovered_at_import():
    """The seeded module is fetched at import time and exposed as a class."""
    classes = _dynamic_module_classes()
    assert classes, f"No dynamic module classes were created; dir(infraweave)={dir(infraweave)}"
    names = {n.lower() for n in classes}
    assert SEED_MODULE in names, f"Expected '{SEED_MODULE}' among discovered classes {sorted(names)}"


def test_get_latest_version_by_name():
    """The low-level Module API resolves the seeded version from the backend."""
    module = infraweave.Module.get_latest_version_by_name(SEED_MODULE, SEED_TRACK)
    assert module.version == SEED_VERSION
    assert module.track == SEED_TRACK


def test_instantiate_dynamic_module_class():
    """The generated wrapper class fetches the pinned version from the backend."""
    classes = _dynamic_module_classes()
    cls = next(obj for name, obj in classes.items() if name.lower() == SEED_MODULE)
    instance = cls(version=SEED_VERSION, track=SEED_TRACK)
    assert instance.version == SEED_VERSION
    assert instance.track == SEED_TRACK


def test_unknown_version_raises():
    """An unknown module surfaces an error rather than succeeding."""
    # The SDK panics here; pyo3 maps that to PanicException, which subclasses
    # BaseException (not Exception), so don't narrow this to Exception.
    with pytest.raises(BaseException):
        infraweave.Module.get_latest_version_by_name("definitely-not-a-real-module", SEED_TRACK)


@pytest.mark.skipif(
    os.environ.get("INFRAWEAVE_TEST_APPLY") != "1",
    reason="apply()/destroy() block on the runner, which only the Rust harness "
    "impersonates (it sets INFRAWEAVE_TEST_APPLY=1).",
)
def test_deployment_lifecycle():
    """Drive apply() end-to-end. The harness mocks the runner, marking the
    deployment Successful so apply() and the auto-destroy on exit return."""
    region = os.environ.get("AWS_REGION", "us-west-2")
    classes = _dynamic_module_classes()
    cls = next(obj for name, obj in classes.items() if name.lower() == SEED_MODULE)
    module = cls(version=SEED_VERSION, track=SEED_TRACK)

    deployment = infraweave.Deployment(
        name="pytest-bucket",
        namespace="dev",
        region=region,
        module=module,
    )

    with deployment:
        deployment.set_variables(bucket_name="pytest-bucket-abc123")
        result = deployment.apply()
        assert result is not None
        assert result.deployment_id == "s3bucketsimple/pytest-bucket"
        assert result.region == region
