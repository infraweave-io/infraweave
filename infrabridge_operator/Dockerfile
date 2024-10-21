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
COPY ./infrabridge_operator/Cargo.toml /app/infrabridge_operator/
COPY ./infrabridge_operator/Cargo.lock /app/infrabridge_operator/
COPY ./infrabridge_operator/src /app/infrabridge_operator/src/

RUN cd infrabridge_operator && cargo build --release

FROM rust:slim-bullseye

COPY --from=builder /app/infrabridge_operator/target/release/infrabridge_operator /usr/local/bin/infrabridge_operator

CMD ["infrabridge_operator"]