FROM rust:bookworm AS builder

WORKDIR /src
COPY search/ search/
RUN cargo build --release --manifest-path search/Cargo.toml

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/search/target/release/search-server /usr/local/bin/

ENV DATA_DIR=/data/index
ENV BIND_ADDR=0.0.0.0:3001

CMD ["sh", "-c", "exec search-server \"$DATA_DIR\" \"$BIND_ADDR\""]
