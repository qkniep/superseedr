
# SPDX-FileCopyrightText: 2025 The superseedr Contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# syntax=docker/dockerfile:1

# --- Stage 1: The Cross-Builder ---
FROM --platform=$BUILDPLATFORM rust:1-bookworm AS builder

ARG TARGETPLATFORM
ARG TARGETARCH
ARG BUILDPLATFORM
ARG PRIVATE_BUILD=false

# 1. Install 'xx' - A Docker helper for seamless cross-compilation
COPY --from=tonistiigi/xx / /

# 2. Install Native Tools (Clang, LLD, pkg-config)
# These run on the build machine (Intel), so we use standard apt-get
RUN apt-get update && apt-get install -y clang lld pkg-config git

# 3. Install TARGET Libraries (OpenSSL for ARM64/AMD64)
# [CRITICAL FIX] We use 'xx-apt-get' here. 
# This magic command downloads the library for the TARGET architecture (e.g. arm64),
# not the build architecture.
RUN xx-apt-get install -y libssl-dev gcc

WORKDIR /app

# 4. Copy source files
COPY Cargo.toml Cargo.lock ./
COPY ./src ./src

# 5. Build with xx-cargo
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

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y ca-certificates libssl3 && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/superseedr /usr/local/bin/superseedr

ENTRYPOINT ["/usr/local/bin/superseedr"]
