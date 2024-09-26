FROM rust:slim-bullseye AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev wget unzip

RUN mkdir /app
WORKDIR /app

RUN wget https://releases.hashicorp.com/terraform/1.5.7/terraform_1.5.7_linux_arm64.zip \
    && unzip terraform_1.5.7_linux_arm64.zip \
    && mv terraform /usr/local/bin/terraform

COPY ./terraform_runner/Cargo.toml /app/terraform_runner/
COPY ./terraform_runner/Cargo.lock /app/terraform_runner/
COPY ./terraform_runner/src /app/terraform_runner/src/
COPY ./environments/ /app/environments/


RUN cd terraform_runner && cargo build --release

FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y ca-certificates

RUN mkdir /app
WORKDIR /app

COPY --from=builder /usr/local/bin/terraform /usr/local/bin/terraform
COPY --from=builder /app/terraform_runner/target/release/terraform_runner /usr/local/bin/terraform_runner

CMD ["terraform_runner"]