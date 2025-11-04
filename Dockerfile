# syntax=docker/dockerfile:1

# Build stage
FROM rust:alpine AS builder

# Install build dependencies (no openssl-libs-static to avoid full static linking)
RUN apk add --no-cache \
    musl-dev \
    zeromq-dev \
    boost-dev \
    openssl-dev \
    pkgconfig \
    cmake \
    make \
    g++ \
    gcc \
    perl \
    linux-headers \
    bash \
    brotli \
    sqlite-dev \
    git \
    tcl \
    curl \
    wget \
    unzip \
    pcre2-dev

# Set working directory
WORKDIR /build

# Build sqlean extensions for musl/Alpine
WORKDIR /tmp

# Build from source - pre-built binaries are glibc
RUN git clone --depth 1 --branch 0.28.0 https://github.com/nalgeon/sqlean.git && \
    cd sqlean && \
    make download-sqlite && \
    make download-external && \
    make prepare-dist && \
    mkdir -p /build/sqlean-musl && \
    echo "Building crypto extension for musl..." && \
    gcc -O3 -Isrc -DSQLEAN_VERSION='"0.28.0"' -z now -z relro -Wall -Wsign-compare -Wno-unknown-pragmas -fPIC -shared \
       src/sqlite3-crypto.c src/crypto/*.c \
       -o dist/crypto.so && \
    echo "Building regexp extension for musl..." && \
    gcc -O3 -Isrc -DSQLEAN_VERSION='"0.28.0"' -z now -z relro -Wall -Wsign-compare -Wno-unknown-pragmas -fPIC -shared \
       -include src/regexp/constants.h src/sqlite3-regexp.c src/regexp/*.c src/regexp/pcre2/*.c \
       -o dist/regexp.so && \
    cp dist/crypto.so dist/regexp.so /build/sqlean-musl/ && \
    echo "Built musl extensions successfully!" && \
    ls -la /build/sqlean-musl/

# Copy the entire workspace
WORKDIR /build
COPY . .

# Replace the glibc sqlean extensions with musl versions
RUN echo "Replacing glibc extensions with musl versions..." && \
    ls -la /build/sqlean-musl/ && \
    rm -rf core/indexer/sqlean-0.28.0/linux-* && \
    mkdir -p core/indexer/sqlean-0.28.0/linux-x64 && \
    mkdir -p core/indexer/sqlean-0.28.0/linux-arm64 && \
    cp /build/sqlean-musl/*.so core/indexer/sqlean-0.28.0/linux-x64/ && \
    cp /build/sqlean-musl/*.so core/indexer/sqlean-0.28.0/linux-arm64/ && \
    echo "Replaced extensions:" && \
    ls -la core/indexer/sqlean-0.28.0/linux-x64/

# Add wasm32-unknown-unknown target and install required tools
RUN rustup target add wasm32-unknown-unknown && \
    cargo install wasm-opt

# Build the indexer binary in release mode
# Build from core directory since it's a workspace member
# Force dynamic linking (not fully static) to allow dlopen() of .so extensions
WORKDIR /build/core
ENV RUSTFLAGS="-C target-feature=-crt-static"
RUN cargo build --release --package indexer

# Runtime stage
FROM alpine:latest

# Install runtime dependencies (including pcre2 for regexp.so)
RUN apk add --no-cache \
    zeromq \
    boost-libs \
    ca-certificates \
    libgcc \
    libstdc++ \
    pcre2

# Create a non-root user to run the application
RUN addgroup -g 1000 kontor && \
    adduser -D -u 1000 -G kontor kontor

# Create data directory with proper permissions
RUN mkdir -p /data && chown -R kontor:kontor /data

# Copy the built binary from builder stage
COPY --from=builder /build/core/target/release/kontor /usr/local/bin/kontor

# Set proper permissions for the binary
RUN chmod +x /usr/local/bin/kontor

# Switch to non-root user
USER kontor

# Set working directory
WORKDIR /home/kontor

# Set default environment variables (can be overridden)
ENV DATA_DIR=/data \
    API_PORT=9333 \
    ZMQ_ADDRESS=tcp://127.0.0.1:28332 \
    NETWORK=bitcoin \
    STARTING_BLOCK_HEIGHT=921300 \
    USE_LOCAL_REGTEST=false \
    BITCOIN_RPC_USER=rpc \
    BITCOIN_RPC_PASSWORD=rpc

# Expose the API port (default 9333, override with -p flag in docker run)
EXPOSE 9333

# Volume for persistent data
VOLUME ["/data"]

# Run the indexer
# All configuration can be passed via environment variables or CLI args
# Examples:
#   docker run -e NETWORK=testnet -e API_PORT=9334 kontor
#   docker run kontor --network testnet --api-port 9334
ENTRYPOINT ["/usr/local/bin/kontor"]
