# Built from the repository root, which holds the shared crate:
#   docker build -f server/Dockerfile -t flexspeak-monster .

FROM rust:1.99-trixie AS build
WORKDIR /src

# Dependencies first, against a stand-in for the server's code, so changing only the server
# reuses this layer.
COPY crates/monster-core crates/monster-core
COPY server/Cargo.toml server/Cargo.lock server/
RUN mkdir server/src \
    && echo 'fn main() {}' > server/src/main.rs \
    && touch server/src/lib.rs \
    && cd server \
    && cargo build --release --locked \
    && rm -rf src

COPY server/src server/src
# COPY keeps the files' own times, which can be older than the stand-in's build.
RUN cd server \
    && touch src/main.rs src/lib.rs \
    && cargo build --release --locked

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --user-group --home-dir /data --shell /usr/sbin/nologin monster \
    && mkdir /data \
    && chown monster:monster /data
COPY --from=build /src/server/target/release/flexspeak-monster /usr/local/bin/flexspeak-monster

USER monster
ENV PORT=8787 DATA_DIR=/data
VOLUME /data
EXPOSE 8787
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["flexspeak-monster", "healthcheck"]
CMD ["flexspeak-monster"]
