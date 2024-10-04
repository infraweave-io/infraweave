FROM rust:slim-bullseye AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev wget unzip

RUN mkdir /app
WORKDIR /app

RUN wget https://releases.hashicorp.com/terraform/1.5.7/terraform_1.5.7_linux_arm64.zip \
    && unzip terraform_1.5.7_linux_arm64.zip \
    && mv terraform /usr/local/bin/terraform

RUN wget https://github.com/open-policy-agent/opa/releases/download/v0.69.0/opa_linux_arm64_static \
-O /usr/local/bin/opa && chmod +x /usr/local/bin/opa

COPY ./terraform_runner/Cargo.toml /app/terraform_runner/
COPY ./terraform_runner/Cargo.lock /app/terraform_runner/
COPY ./terraform_runner/src /app/terraform_runner/src/
COPY ./env_aws/ /app/env_aws/
COPY ./env_azure/ /app/env_azure/
COPY ./env_common/ /app/env_common/
COPY ./defs/ /app/defs/
COPY ./utils/ /app/utils/

RUN cd terraform_runner && cargo build --release

FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y ca-certificates

RUN mkdir /app
WORKDIR /app

COPY --from=builder /usr/local/bin/terraform /usr/local/bin/terraform
COPY --from=builder /usr/local/bin/opa /usr/local/bin/opa
COPY --from=builder /app/terraform_runner/target/release/terraform_runner /usr/local/bin/terraform_runner

CMD ["terraform_runner"]