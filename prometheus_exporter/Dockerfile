FROM rust:slim-bullseye AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev wget unzip make

RUN mkdir /app
WORKDIR /app

COPY ./prometheus_exporter/Cargo.toml /app/prometheus_exporter/
COPY ./Cargo.lock /app/prometheus_exporter/
COPY ./prometheus_exporter/src /app/prometheus_exporter/src/
COPY ./env_aws/ /app/env_aws/
COPY ./env_azure/ /app/env_azure/
COPY ./env_common/ /app/env_common/
COPY ./defs/ /app/defs/
COPY ./utils/ /app/utils/

RUN cd prometheus_exporter && cargo build --release

# Create empty directory to copy to final image
RUN mkdir /app/app

FROM gcr.io/distroless/cc-debian12

# Copy empty directory since mkdir doesn't work in distroless
COPY --from=builder /app/app /app 

WORKDIR /app

COPY --from=builder /app/prometheus_exporter/target/release/prometheus_exporter /usr/local/bin/prometheus_exporter

EXPOSE 3001

CMD ["prometheus_exporter"]