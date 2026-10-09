# Builds the example app: the trunk-built WASM client plus the axum server
# that serves it. Apps built on the framework copy this file and change the
# package names (example-client / example-server). See docs/deploy.md.

FROM rust:1-slim-bookworm AS builder

ARG TRUNK_VERSION=0.21.14
ARG TRUNK_SHA256=f2b4680cd239693a646a2795e4633c625328d7b2a044fbe749fa3a2fe9e7036b

RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN rustup target add wasm32-unknown-unknown
# Prebuilt, checksum-verified trunk (compiling it from source is slow).
RUN curl -fsSL -o /tmp/trunk.tar.gz \
        "https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-x86_64-unknown-linux-gnu.tar.gz" \
    && echo "${TRUNK_SHA256}  /tmp/trunk.tar.gz" | sha256sum -c - \
    && tar -xzf /tmp/trunk.tar.gz -C /usr/local/bin \
    && rm /tmp/trunk.tar.gz

WORKDIR /app
COPY . .
# Client first: outputs build/dist (see example/client/Trunk.toml).
RUN cd example/client && trunk build --release
RUN cargo build --release -p example-server

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/example-server ./example-server
COPY --from=builder /app/build/dist ./dist
ENV STATIC_DIR=/app/dist \
    BIND_ADDR=0.0.0.0:8080
EXPOSE 8080
CMD ["./example-server"]
