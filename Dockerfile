
# SPDX-FileCopyrightText: 2025 The superseedr Contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# syntax=docker/dockerfile:1

# --- Stage 1: The Cross-Builder ---
# We use the build node's NATIVE architecture to run the compiler fast.
FROM --platform=$BUILDPLATFORM rust:1-bookworm AS builder

# These ARGs are automatically populated by Docker Buildx
ARG TARGETPLATFORM
ARG TARGETARCH
ARG BUILDPLATFORM
ARG PRIVATE_BUILD=false

# 1. Install 'xx' - A Docker helper for seamless cross-compilation
COPY --from=tonistiigi/xx / /

# 2. Install Clang/LLD AND OpenSSL dependencies
RUN apt-get update && apt-get install -y clang lld pkg-config libssl-dev

WORKDIR /app

# 3. Copy source files
COPY Cargo.toml Cargo.lock ./
COPY ./src ./src

# 4. Build with xx-cargo
RUN --mount=type=cache,target=/usr/local/cargo/git/db \
    --mount=type=cache,target=/usr/local/cargo/registry/cache \
    --mount=type=cache,target=/usr/local/cargo/registry/index \
    --mount=type=cache,target=/app/target \
    TRIPLE=$(xx-cargo --print-target-triple) && \
    if [ "$PRIVATE_BUILD" = "true" ]; then \
        xx-cargo build --release --no-default-features --target "$TRIPLE" --target-dir ./target; \
    else \
        xx-cargo build --release --target "$TRIPLE" --target-dir ./target; \
    fi && \
    cp ./target/$TRIPLE/release/superseedr /app/superseedr

# --- Stage 2: The Final Image ---
FROM debian:bookworm-slim AS final

RUN apt-get update && \
    apt-get install -y ca-certificates libssl3 && \
    rm -rf /var/lib/apt/lists/*

# Copy the compiled binary from the builder stage
COPY --from=builder /app/superseedr /usr/local/bin/superseedr

ENTRYPOINT ["/usr/local/bin/superseedr"]
