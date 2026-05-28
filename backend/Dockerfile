# ── Build stage ──────────────────────────────────────────────────────────────
FROM rust:1.86-slim-bookworm AS builder

WORKDIR /app

# Install system deps for native-tls
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Build actual source
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY migrations ./migrations
RUN cargo build --release

# ── Runtime stage ─────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/hyperliquid-lens ./hyperliquid-lens
COPY --from=builder /app/migrations ./migrations

EXPOSE 3001

CMD ["./hyperliquid-lens"]
