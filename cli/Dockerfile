FROM rust:1.87-alpine AS builder

ARG VERSION
ENV VERSION=${VERSION}

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

COPY . /app/

RUN cargo build --release -p cli

# Copy jq binary from jq image to inherit architecture
FROM ghcr.io/jqlang/jq:1.7.1 AS jq

FROM alpine:3.22

RUN apk --no-cache update && \
    apk --no-cache add git ca-certificates libcrypto3 libssl3

COPY --from=jq /jq /usr/local/bin/jq

WORKDIR /app

COPY --from=builder /app/target/release/cli /usr/local/bin/cli

CMD ["cli"]