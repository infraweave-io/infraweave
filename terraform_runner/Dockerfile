# Use an argument for architecture; defaults to amd64
ARG ARCH=amd64

FROM rust:1.82-alpine AS builder
ARG ARCH

RUN apk add --no-cache \
    build-base \
    musl-dev \
    openssl-dev \
    wget \
    unzip \
    make \
    pkgconfig \
    perl

RUN mkdir /app
WORKDIR /app

# Download Terraform binary for the specified architecture
RUN wget https://releases.hashicorp.com/terraform/1.5.7/terraform_1.5.7_linux_${ARCH}.zip \
    && unzip terraform_1.5.7_linux_${ARCH}.zip \
    && mv terraform /usr/local/bin/terraform

# Download OPA binary for the specified architecture
RUN wget https://github.com/open-policy-agent/opa/releases/download/v0.69.0/opa_linux_${ARCH}_static \
    -O /usr/local/bin/opa && chmod +x /usr/local/bin/opa

COPY ./terraform_runner/Cargo.toml /app/terraform_runner/
COPY ./Cargo.lock /app/
COPY ./terraform_runner/src /app/terraform_runner/src/
COPY ./env_aws/ /app/env_aws/
COPY ./env_azure/ /app/env_azure/
COPY ./env_common/ /app/env_common/
COPY ./defs/ /app/defs/
COPY ./utils/ /app/utils/

RUN rustup target add aarch64-unknown-linux-musl

RUN cd terraform_runner && cargo build --release

FROM alpine:3.20

RUN apk --no-cache update && \
    apk --no-cache add git libcrypto3 libssl3

WORKDIR /app
RUN adduser -u 1000 -D -h /home/runner runner
RUN chown runner:runner /app

COPY --from=builder /usr/local/bin/terraform /usr/local/bin/terraform
COPY --from=builder /usr/local/bin/opa /usr/local/bin/opa
COPY --from=builder /app/terraform_runner/target/release/terraform_runner /usr/local/bin/terraform_runner

RUN chown -R 1000:1000 /app

USER runner

CMD ["terraform_runner"]