FROM rust:slim-bullseye AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev wget unzip make curl

RUN mkdir /app
WORKDIR /app

COPY ./webserver-openapi/Cargo.toml /app/webserver-openapi/
COPY ./Cargo.lock /app/webserver-openapi/
COPY ./webserver-openapi/src /app/webserver-openapi/src/
COPY ./env_aws/ /app/env_aws/
COPY ./env_azure/ /app/env_azure/
COPY ./env_common/ /app/env_common/
COPY ./defs/ /app/defs/
COPY ./utils/ /app/utils/

RUN cd webserver-openapi && cargo build --release

# Create empty directory to copy to final image
RUN mkdir /app/app

FROM gcr.io/distroless/cc-debian12

# Copy empty directory since mkdir doesn't work in distroless
COPY --from=builder /app/app /app 

WORKDIR /app

COPY --from=builder /app/webserver-openapi/target/release/webserver-openapi /usr/local/bin/webserver-openapi

EXPOSE 8081

CMD ["webserver-openapi"]