# syntax=docker/dockerfile:1

# Planner stage - generates dependency recipe
FROM rust:alpine AS planner
RUN apk add --no-cache musl-dev
RUN cargo install cargo-chef
WORKDIR /build
COPY core core
WORKDIR /build/core
RUN cargo chef prepare --recipe-path recipe.json

# Builder base - builds dependencies once and caches them
FROM rust:alpine AS builder-base

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

RUN cargo install cargo-chef
RUN rustup target add wasm32-unknown-unknown
RUN cargo install wasm-opt

WORKDIR /build

# Cacher stage - builds dependencies (cached separately from source code)
FROM builder-base AS cacher
WORKDIR /build/core
COPY --from=planner /build/core/recipe.json recipe.json
ENV RUSTFLAGS="-C target-feature=-crt-static"
RUN cargo chef cook --release --recipe-path recipe.json

# Builder stage - builds actual code
FROM builder-base AS builder

# Copy vendored sqlean source to build extensions for musl/Alpine
WORKDIR /tmp
COPY core/indexer/sqlean-0.28.0 sqlean
RUN cd sqlean && \
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
    cp dist/crypto.so dist/regexp.so /build/sqlean-musl/

WORKDIR /build

# Copy cached dependencies from cacher stage
COPY --from=cacher /build/core/target /build/core/target
COPY --from=cacher /usr/local/cargo /usr/local/cargo

# Copy source code
COPY . .

# Replace the glibc sqlean extensions with musl versions
RUN rm -rf core/indexer/sqlean-0.28.0/linux-* && \
    mkdir -p core/indexer/sqlean-0.28.0/linux-x64 && \
    mkdir -p core/indexer/sqlean-0.28.0/linux-arm64 && \
    cp /build/sqlean-musl/*.so core/indexer/sqlean-0.28.0/linux-x64/ && \
    cp /build/sqlean-musl/*.so core/indexer/sqlean-0.28.0/linux-arm64/

# Build only the indexer (dependencies already built)
WORKDIR /build/core
ENV RUSTFLAGS="-C target-feature=-crt-static"
RUN cargo build --release --package indexer

# Runtime stage
FROM alpine:latest

RUN apk add --no-cache \
    zeromq \
    boost-libs \
    ca-certificates \
    libgcc \
    libstdc++ \
    pcre2

RUN addgroup -g 1000 kontor && \
    adduser -D -u 1000 -G kontor kontor

RUN mkdir -p /data && chown -R kontor:kontor /data

COPY --from=builder /build/core/target/release/kontor /usr/local/bin/kontor
RUN chmod +x /usr/local/bin/kontor

USER kontor
WORKDIR /home/kontor

ENV DATA_DIR=/data \
    API_PORT=9333 \
    ZMQ_ADDRESS=tcp://127.0.0.1:28332 \
    NETWORK=bitcoin \
    STARTING_BLOCK_HEIGHT=921300 \
    USE_LOCAL_REGTEST=false \
    BITCOIN_RPC_USER=rpc \
    BITCOIN_RPC_PASSWORD=rpc

EXPOSE 9333
VOLUME ["/data"]
ENTRYPOINT ["/usr/local/bin/kontor"]
