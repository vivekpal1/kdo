FROM rust:1.82-slim-bookworm AS builder

WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN cargo build --release -p kdo-cli \
    && strip target/release/kdo

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    git \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/kdo /usr/local/bin/kdo

WORKDIR /workspace
ENTRYPOINT ["kdo"]
CMD ["--help"]
