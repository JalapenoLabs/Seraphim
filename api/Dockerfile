# --- Build stage -------------------------------------------------------------
# Pinned to 1.88: some transitive deps ship edition2024 crates.
FROM rust:1.88-slim AS builder
WORKDIR /app

# ring (via rustls) and mimalloc compile C, so a C toolchain is required.
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Pre-fetch dependencies in their own layer for fast incremental rebuilds.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
RUN cargo fetch

# Migrations must be present: `sqlx::migrate!` embeds them at compile time.
COPY src/ ./src/
COPY migrations/ ./migrations/
RUN cargo build --release

# --- Runtime stage -----------------------------------------------------------
FROM debian:bookworm-slim AS runtime
WORKDIR /app

# ca-certificates: TLS to GitHub. wget: container healthcheck.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates wget \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/seraphim-api /usr/local/bin/seraphim-api

EXPOSE 8080
CMD ["seraphim-api"]
