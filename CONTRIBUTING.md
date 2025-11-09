# Contributing to Infraweave

Thank you for your interest in contributing to Infraweave! We value your contributions and strive to make the process clear and collaborative.

## Getting Started

1. **Fork and Clone**:
   - Fork this repository and clone your fork locally:
     ```bash
     git clone https://github.com/infraweave-io/infraweave.git
     cd infraweave
     ```

2. **Set Up Your Environment**:
   - Ensure you have the latest version of Rust installed: https://www.rust-lang.org/tools/install
   - Install dependencies:
     ```bash
     cargo build
     ```

3. **Run Tests**:
   - Verify everything is working before making changes:
     ```bash
     make test
     ```
     Note that the integration-tests requires docker.

     > If you are on a mac with apple silicon you can use `colima start --cpu 5 --memory 10 --arch aarch64 --vm-type=vz --vz-rosetta --mount-type virtiofs`

   - To run a specific integration test:
     ```bash
     make aws-integration-tests test=test_module_deprecation_existing_deployment_can_modify
     ```

4. **Explore the Code**:
   - Familiarize yourself with the project structure and documentation.

## Contribution Workflow

1. **Open an Issue**:
   - Before starting any work, please [create an issue](https://github.com/infraweave-io/infraweave/issues) to discuss your idea or bug fix.
   - Provide details and context for your proposed changes.

2. **Create a Branch**:
   - Use a descriptive branch name for your work:
     ```bash
     git checkout -b feature/add-new-feature
     ```

3. **Make Changes**:
   - Write clear, modular, and well-documented code.
   - Format your code using:
     ```bash
     cargo fmt
     ```
   - Check for linting issues:
     ```bash
     cargo clippy
     ```
   - Add or update tests where necessary.

4. **Submit a Pull Request (PR)**:
   - Push your branch to your fork:
     ```bash
     git push origin feature/add-new-feature
     ```
   - Open a PR against the `main` branch of this repository.
   - In your PR description, include:
     - A link to the issue being resolved.
     - A summary of your changes.
     - Any additional context or screenshots, if applicable.

5. **Address Feedback**:
   - The maintainers will review your PR and may request changes. Please address feedback promptly.

## Contributor License Agreement (CLA)

By contributing to Infraweave, you agree that your contributions will be licensed under the [Apache 2.0 License](LICENSE). If you are contributing on behalf of your employer, ensure that you have the necessary permissions to do so.

## Style Guidelines

- Follow Rust's best practices and style conventions.
- Use the tools provided:
  - **Formatter**: `cargo fmt`
  - **Linter**: `cargo clippy` (TODO: needs improvements in existing code)

## Reporting Issues

If you encounter a bug or have a feature request, please [open an issue](https://github.com/infraweave-io/infraweave/issues). Include:
- A clear and concise title and description.
- Steps to reproduce (if applicable).
- Expected vs. actual behavior.

## Security Vulnerabilities

If you discover a security vulnerability in Infraweave, please report it privately to [opensource@infraweave.com](mailto:opensource@infraweave.com). We will respond promptly and work with you to resolve the issue.

## Code of Conduct

We are committed to providing a welcoming and inclusive environment for all contributors. Please read and follow our [Code of Conduct](CODE_OF_CONDUCT.md).

---

Thank you for contributing to Infraweave!
