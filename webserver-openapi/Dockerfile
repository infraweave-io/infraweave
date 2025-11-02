FROM rust:slim-bookworm AS chef
RUN apt-get update && apt-get install -y pkg-config libssl-dev git && rm -rf /var/lib/apt/lists/* && \
    cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
RUN apt-get update && apt-get install -y wget unzip make curl && rm -rf /var/lib/apt/lists/*

# Build dependencies - this layer is cached unless Cargo.toml/Cargo.lock changes
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --package webserver-openapi --recipe-path recipe.json

# Build application - only this runs when source code changes
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