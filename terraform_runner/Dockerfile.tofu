# Use an argument for architecture; defaults to amd64
ARG ARCH=amd64

FROM rust:1.88-alpine AS builder
ARG ARCH

RUN apk add --no-cache \
    build-base \
    musl-dev \
    openssl-dev \
    wget \
    unzip \
    make \
    pkgconfig \
    perl \
    git

RUN mkdir /app
WORKDIR /app

# Download Tofu binary for the specified architecture
RUN wget https://github.com/opentofu/opentofu/releases/download/v1.9.1/tofu_1.9.1_linux_${ARCH}.zip \
    && unzip tofu_1.9.1_linux_${ARCH}.zip \
    && mv tofu /usr/local/bin/terraform

# Download OPA binary for the specified architecture
RUN wget https://github.com/open-policy-agent/opa/releases/download/v0.69.0/opa_linux_${ARCH}_static \
    -O /usr/local/bin/opa && chmod +x /usr/local/bin/opa

COPY . /app/

RUN rustup target add aarch64-unknown-linux-musl

RUN cargo build --release -p terraform_runner

FROM alpine:3.20

RUN apk --no-cache update && \
    apk --no-cache add git libcrypto3 libssl3

WORKDIR /app
RUN adduser -u 1000 -D -h /home/runner runner
RUN chown runner:runner /app

COPY --from=builder /usr/local/bin/terraform /usr/local/bin/terraform
COPY --from=builder /usr/local/bin/opa /usr/local/bin/opa
COPY --from=builder /app/target/release/terraform_runner /usr/local/bin/terraform_runner

RUN chown -R 1000:1000 /app

USER runner

CMD ["terraform_runner"]