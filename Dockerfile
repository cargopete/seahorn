# ── Stage 1: build ────────────────────────────────────────────────────────────
FROM rust:slim-bookworm AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

COPY . .

RUN cargo build --release -p seahorn -p seahorn-gateway

# ── Stage 2: seahorn indexer ───────────────────────────────────────────────────
FROM debian:bookworm-slim AS seahorn

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/seahorn /usr/local/bin/seahorn

ENTRYPOINT ["seahorn"]

# ── Stage 3: payment gateway ───────────────────────────────────────────────────
FROM debian:bookworm-slim AS gateway

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/seahorn-gateway /usr/local/bin/seahorn-gateway

ENTRYPOINT ["seahorn-gateway"]
