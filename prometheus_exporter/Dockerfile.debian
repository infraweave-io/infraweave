FROM gcr.io/distroless/cc-debian12
ARG TARGETOS
ARG TARGETARCH

COPY binaries/prometheus_exporter-${TARGETOS}-${TARGETARCH} /usr/local/bin/prometheus_exporter

WORKDIR /app

EXPOSE 3001

CMD ["prometheus_exporter"]