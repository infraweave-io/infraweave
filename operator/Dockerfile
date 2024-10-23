FROM rust:slim-bullseye AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev

RUN mkdir /app
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

RUN cd operator && cargo build --release

FROM rust:slim-bullseye

COPY --from=builder /app/operator/target/release/operator /usr/local/bin/operator

CMD ["operator"]