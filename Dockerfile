# syntax=docker/dockerfile:1.7

# Toolchain pin — must match rust-toolchain.toml (checked by scripts/check-toolchain-pin.sh).
FROM rust:1.98.0-bookworm AS builder
WORKDIR /src

# The whole workspace is needed: the `neoc` bin pulls neoc-vm + the neoc-*
# compiler libs, and neoc-grammar's build.rs cc-compiles the tree-sitter-neoc
# parser.c. `.dockerignore` keeps target/, .git, and the script
# tests out of the context. BuildKit cache mounts give dep + incremental
# caching without the brittle per-crate stub trick (target/ is a cache mount,
# so the binary is copied out before the RUN ends).
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --bin neoc \
    && cp target/release/neoc /usr/local/bin/neoc \
    && strip /usr/local/bin/neoc

FROM debian:bookworm-slim AS runner
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/bin/neoc /usr/local/bin/neoc
WORKDIR /app
ENTRYPOINT ["neoc"]
