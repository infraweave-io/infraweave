FROM rust:slim-bookworm AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev wget unzip make curl git \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir /app
WORKDIR /app

COPY . /app/

RUN cargo build --release -p webserver-openapi

# Create empty directory to copy to final image
RUN mkdir /app/app

FROM gcr.io/distroless/cc-debian12

# Copy empty directory since mkdir doesn't work in distroless
COPY --from=builder /app/app /app 

WORKDIR /app

COPY --from=builder /app/target/release/webserver-openapi /usr/local/bin/webserver-openapi

EXPOSE 8081

CMD ["webserver-openapi"]