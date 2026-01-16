FROM alpine:3.20
ARG TARGETOS
ARG TARGETARCH

RUN apk --no-cache update && \
    apk --no-cache add git ca-certificates libcrypto3 libssl3 jq

WORKDIR /app

COPY binaries/cli-${TARGETOS}-${TARGETARCH}-musl /usr/local/bin/cli

CMD ["cli"]