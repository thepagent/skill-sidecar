# ── Build ────────────────────────────────────────────────────────────────────
FROM rust:1-slim AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

# ── Runtime ──────────────────────────────────────────────────────────────────
FROM debian:trixie-slim
RUN apt-get update && apt-get install -y ca-certificates procps && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/skill-sidecar /usr/local/bin/skill-sidecar
RUN ln -s /usr/bin/uptime /usr/local/bin/uptime
EXPOSE 8080
CMD ["skill-sidecar"]
