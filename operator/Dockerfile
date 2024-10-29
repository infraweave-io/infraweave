# syntax=docker/dockerfile:1.4

FROM rust:slim-bullseye AS builder

ENV K8S_OPENAPI_ENABLED_VERSION=v1.27

RUN apt-get update && apt-get install -y pkg-config libssl-dev make

WORKDIR /app

COPY ./env_aws /app/env_aws/
COPY ./env_azure /app/env_azure/
COPY ./defs /app/defs/
COPY ./utils /app/utils/
COPY ./env_common /app/env_common/
COPY ./crd-templator /app/crd-templator/
COPY ./operator/Cargo.toml /app/operator/
COPY ./operator/Cargo.lock /app/operator/
COPY ./operator/src /app/operator/src/

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cd operator && cargo build --release

FROM gcr.io/distroless/cc-debian12

COPY --from=builder /app/operator/target/release/operator /usr/local/bin/operator

CMD ["operator"]
