# InfraWeave-Py

This package is a minimal python wrapper to interface with InfraWeave to set up you modules and stacks

> Note: this is a preview version

Read the [docs here](https://infraweave-io.github.io/infraweave/infraweave.html)

## Integration testing against a local backend

There's an integration test that builds the compiled module with maturin and
runs it against a local backend (DynamoDB + MinIO via testcontainers, seeded
with a sample provider and module, `aws_direct` mode). Everything runs in
Docker, so the host only needs Docker — no Python or maturin. From the
workspace root:

```bash
make python-sdk-integration-test
```

The Rust harness ([../integration-tests/tests/python_sdk.rs](../integration-tests/tests/python_sdk.rs))
starts and seeds the backend, then runs a toolchain image
([Dockerfile.itest](Dockerfile.itest)) that builds the module and runs
[tests/](tests/) via pytest. The test container reaches the backend over the
Docker bridge IP, and cargo's registry/`target` dirs are cached in named
volumes across runs.
