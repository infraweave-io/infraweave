FROM rust:1.88-alpine AS chef
RUN apk add --no-cache build-base musl-dev openssl-dev pkgconfig perl git && \
    cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

ARG VERSION
ENV VERSION=${VERSION}

RUN apk add --no-cache wget unzip make

# Build dependencies - this layer is cached unless Cargo.toml/Cargo.lock changes
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --package cli --recipe-path recipe.json

# Build application - only this runs when source code changes
COPY . /app/
RUN cargo build --release -p cli

# Copy jq binary from jq image to inherit architecture
FROM ghcr.io/jqlang/jq:1.7.1 AS jq

FROM alpine:3.20

RUN apk --no-cache update && \
    apk --no-cache add git ca-certificates libcrypto3 libssl3

COPY --from=jq /jq /usr/local/bin/jq

WORKDIR /app

COPY --from=builder /app/target/release/cli /usr/local/bin/cli

CMD ["cli"]