FROM gcr.io/distroless/cc-debian12
ARG TARGETOS
ARG TARGETARCH

COPY binaries/webserver-openapi-${TARGETOS}-${TARGETARCH} /usr/local/bin/webserver-openapi

WORKDIR /app

EXPOSE 8081

CMD ["webserver-openapi"]