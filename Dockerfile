# syntax=docker/dockerfile:1
FROM rust:1-slim-bookworm AS build
WORKDIR /src
# Portable build: the repo's .cargo/config.toml uses target-cpu=native, which
# would tie the image to the build machine. SHA-NI is detected at runtime anyway.
ENV RUSTFLAGS="-C opt-level=3"
COPY Cargo.toml ./
COPY Cargo.loc[k] ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN useradd --system --no-create-home miner
COPY --from=build /src/target/release/bitcoin-lottery /usr/local/bin/bitcoin-lottery
USER miner
# Configuration comes from environment variables (see .env.example).
ENTRYPOINT ["bitcoin-lottery"]
