# syntax=docker/dockerfile:1.7

FROM rust:1-bookworm AS builder
WORKDIR /src

COPY Cargo.toml Cargo.lock ./
COPY crates/neoc/Cargo.toml ./crates/neoc/Cargo.toml
COPY crates/neoc-vm/Cargo.toml ./crates/neoc-vm/Cargo.toml
RUN mkdir -p crates/neoc/src crates/neoc-vm/src crates/neoc-vm/benches/lib crates/neoc-vm/benches/vnd \
    && echo "fn main() {}" > crates/neoc/src/main.rs \
    && echo "" > crates/neoc-vm/src/lib.rs \
    && echo "fn main() {}" > crates/neoc-vm/benches/lib/lib.rs \
    && echo "fn main() {}" > crates/neoc-vm/benches/vnd/vnd.rs \
    && cargo build --release --bin neoc \
    && rm -rf crates/neoc/src crates/neoc-vm/src crates/neoc-vm/benches

COPY crates/neoc/src ./crates/neoc/src
COPY crates/neoc-vm/src ./crates/neoc-vm/src
COPY crates/neoc-vm/benches ./crates/neoc-vm/benches
RUN touch crates/neoc/src/main.rs crates/neoc-vm/src/lib.rs \
    && cargo build --release --bin neoc \
    && strip target/release/neoc

FROM debian:bookworm-slim AS runner
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /src/target/release/neoc /usr/local/bin/neoc
WORKDIR /app
ENTRYPOINT ["neoc"]
