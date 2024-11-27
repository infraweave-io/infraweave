FROM rust:slim-bullseye AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev wget unzip make

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

# Create empty directory to copy to final image
RUN mkdir /app/app

# Copy jq binary from jq image to inherit architecture
FROM ghcr.io/jqlang/jq:1.7.1 AS jq

FROM gcr.io/distroless/cc-debian12

# Copy empty directory since mkdir doesn't work in distroless
COPY --from=builder /app/app /app 
COPY --from=jq /jq /usr/local/bin/jq

WORKDIR /app

COPY --from=builder /app/cli/target/release/cli /usr/local/bin/cli

EXPOSE 8081

CMD ["cli"]