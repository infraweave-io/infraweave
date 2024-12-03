FROM rust:1.82-alpine AS builder

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

COPY ./cli/Cargo.toml /app/cli/
COPY ./Cargo.lock /app/cli/
COPY ./cli/src /app/cli/src/
COPY ./env_aws/ /app/env_aws/
COPY ./env_azure/ /app/env_azure/
COPY ./env_common/ /app/env_common/
COPY ./defs/ /app/defs/
COPY ./utils/ /app/utils/

RUN cd cli && cargo build --release

# Copy jq binary from jq image to inherit architecture
FROM ghcr.io/jqlang/jq:1.7.1 AS jq

FROM alpine:3.20

RUN apk --no-cache update && \
    apk --no-cache add git ca-certificates libcrypto3=3.3.2-r1 libssl3=3.3.2-r1

COPY --from=jq /jq /usr/local/bin/jq

WORKDIR /app

COPY --from=builder /app/cli/target/release/cli /usr/local/bin/cli

CMD ["cli"]