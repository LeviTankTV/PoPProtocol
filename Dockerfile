# PoP - Protocol: production image
# Build:  docker build -t popproto .
# Run:    docker compose up
FROM rust:1-slim AS builder
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY client ./client
COPY tools ./tools
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
RUN cargo build --release --bin popproto-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 wget && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/popproto-server /usr/local/bin/popproto-server
ENV PORT=8080 RUST_LOG=info
EXPOSE 8080
HEALTHCHECK --interval=15s --timeout=3s CMD wget -qO- http://127.0.0.1:8080/health || exit 1
CMD ["popproto-server"]
