FROM rust:slim-bookworm AS builder

ENV K8S_OPENAPI_ENABLED_VERSION=v1.33

RUN apt-get update && apt-get install -y pkg-config libssl-dev make git

WORKDIR /app

COPY . /app/

RUN cargo build --release -p operator

FROM gcr.io/distroless/cc-debian12

COPY --from=builder /app/target/release/operator /usr/local/bin/operator

CMD ["operator"]
